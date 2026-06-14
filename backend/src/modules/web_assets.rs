use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap, Response as HttpResponse, StatusCode},
    response::Response,
    Extension, Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        customer::repository::CustomerRepository,
        server_api::{ai_invoke_scope, authenticate_server_key, ServerApiKeyContext},
    },
    state::AppState,
};

pub const MAX_WEB_ASSET_UPLOAD_BYTES: usize = 512 * 1024 * 1024;

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;
const MAX_NAME_LEN: usize = 255;
const MAX_FOLDER_NAME_LEN: usize = 120;
const UPLOAD_TOKEN_PREFIX: &str = "ehup_";
const UPLOAD_TOKEN_DISPLAY_LEN: usize = 18;
const UPLOAD_TOKEN_TTL_MINUTES: i64 = 15;
const UPLOAD_TOKEN_HEADER: &str = "x-entitlehub-upload-token";

#[derive(Debug, Serialize, FromRow)]
pub struct AssetFolder {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    #[sqlx(rename = "metadata_json")]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CustomerAsset {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub ai_asset_id: Option<Uuid>,
    pub name: String,
    pub asset_type: String,
    pub asset_role: String,
    pub source: String,
    pub status: String,
    pub public_url: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub checksum_sha256: Option<String>,
    #[sqlx(rename = "metadata_json")]
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AssetFolderListResponse {
    pub items: Vec<AssetFolder>,
    pub meta: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct AssetFolderResponse {
    pub folder: AssetFolder,
}

#[derive(Debug, Serialize)]
pub struct CustomerAssetListResponse {
    pub items: Vec<CustomerAssetView>,
    pub meta: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct CustomerAssetResponse {
    pub asset: CustomerAssetView,
    #[serde(rename = "assetId")]
    pub asset_id: Uuid,
    pub url: Option<String>,
    #[serde(rename = "type")]
    pub asset_type: String,
    pub kind: String,
    #[serde(rename = "mimeType")]
    pub mime_type: Option<String>,
    #[serde(rename = "thumbnailUrl")]
    pub thumbnail_url: Option<String>,
    pub duration: Option<i64>,
}

impl CustomerAssetResponse {
    fn from_asset(asset: CustomerAsset) -> Self {
        let view = CustomerAssetView::from(asset);
        Self {
            asset_id: view.id,
            url: view.url.clone(),
            asset_type: view.asset_type.clone(),
            kind: view.kind.clone(),
            mime_type: view.mime_type.clone(),
            thumbnail_url: view.thumbnail_url.clone(),
            duration: view.duration,
            asset: view,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CustomerAssetView {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub ai_asset_id: Option<Uuid>,
    pub name: String,
    pub asset_type: String,
    pub kind: String,
    pub asset_role: String,
    pub source: String,
    #[serde(rename = "sourceAlias")]
    pub source_alias: String,
    pub status: String,
    pub public_url: Option<String>,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    #[serde(rename = "mimeType")]
    pub mime_type_alias: Option<String>,
    pub file_size: Option<i64>,
    pub checksum_sha256: Option<String>,
    #[serde(rename = "thumbnailUrl")]
    pub thumbnail_url: Option<String>,
    pub duration: Option<i64>,
    #[serde(rename = "durationSeconds")]
    pub duration_seconds: Option<i64>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    #[serde(rename = "createdAt")]
    pub created_at_alias: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<CustomerAsset> for CustomerAssetView {
    fn from(asset: CustomerAsset) -> Self {
        let thumbnail_url = asset_thumbnail_url(&asset);
        let duration = asset_duration_seconds(&asset.metadata);
        let source_alias = public_source_alias(&asset.source).to_owned();
        Self {
            id: asset.id,
            customer_id: asset.customer_id,
            folder_id: asset.folder_id,
            ai_asset_id: asset.ai_asset_id,
            name: asset.name,
            asset_type: asset.asset_type.clone(),
            kind: asset.asset_type,
            asset_role: asset.asset_role,
            source: asset.source,
            source_alias,
            status: asset.status,
            public_url: asset.public_url.clone(),
            url: asset.public_url,
            mime_type: asset.mime_type.clone(),
            mime_type_alias: asset.mime_type,
            file_size: asset.file_size,
            checksum_sha256: asset.checksum_sha256,
            thumbnail_url,
            duration,
            duration_seconds: duration,
            metadata: asset.metadata,
            created_at: asset.created_at,
            created_at_alias: asset.created_at,
            updated_at: asset.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct CreateAssetUploadResponse {
    pub upload: AssetUploadGrant,
}

#[derive(Debug, Serialize)]
pub struct AssetUploadGrant {
    pub upload_id: Uuid,
    pub method: &'static str,
    pub url: String,
    pub upload_token: String,
    pub token_prefix: String,
    pub expires_at: DateTime<Utc>,
    pub max_bytes: usize,
    pub headers: Value,
}

#[derive(Debug, Serialize)]
pub struct DeleteAssetFolderResponse {
    pub deleted: bool,
    pub folder_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct DeleteCustomerAssetResponse {
    pub deleted: bool,
    pub asset_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct AssetFolderListQuery {
    pub customer_id: Uuid,
    pub parent_id: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAssetFolderRequest {
    pub customer_id: Uuid,
    pub parent_id: Option<Uuid>,
    pub name: String,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAssetFolderRequest {
    pub name: Option<String>,
    pub parent_id: Option<Option<Uuid>>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CustomerAssetListQuery {
    pub customer_id: Uuid,
    pub folder_id: Option<String>,
    pub asset_type: Option<String>,
    pub kind: Option<String>,
    pub asset_role: Option<String>,
    pub source: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAssetUploadRequest {
    pub customer_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub file_name: String,
    pub asset_type: Option<String>,
    pub kind: Option<String>,
    pub asset_role: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct DirectAssetUploadQuery {
    pub customer_id: Uuid,
    pub folder_id: Option<Uuid>,
    pub file_name: String,
    pub asset_type: Option<String>,
    pub kind: Option<String>,
    pub asset_role: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AssetUploadTokenQuery {
    pub token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCustomerAssetRequest {
    pub name: Option<String>,
    pub folder_id: Option<Option<Uuid>>,
    pub asset_role: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, FromRow)]
struct AssetUploadRecord {
    tenant_id: Uuid,
    app_id: Uuid,
    customer_id: Uuid,
    folder_id: Option<Uuid>,
    server_key_id: Uuid,
    file_name: String,
    asset_type: String,
    asset_role: String,
    mime_type: Option<String>,
    file_size: Option<i64>,
    metadata_json: Value,
    expires_at: DateTime<Utc>,
    consumed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, FromRow)]
struct AssetStorageRecord {
    storage_key: Option<String>,
    mime_type: Option<String>,
    file_size: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct GenerationAssetRecord {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub asset_type: String,
    pub asset_role: String,
    pub public_url: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
}

pub async fn list_asset_folders(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<AssetFolderListQuery>,
) -> Result<Json<ApiResponse<AssetFolderListResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, query.customer_id).await?;
    let (filter_parent, parent_id) =
        normalize_optional_uuid_filter(query.parent_id.as_deref(), "parent_id")?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_folders(
        &state,
        &server_key,
        query.customer_id,
        filter_parent,
        parent_id,
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AssetFolderListResponse {
            items,
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

pub async fn create_asset_folder(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<CreateAssetFolderRequest>,
) -> Result<Json<ApiResponse<AssetFolderResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    ensure_folder_belongs(&state, &server_key, payload.customer_id, payload.parent_id).await?;
    let name = normalize_name(&payload.name, "folder name", MAX_FOLDER_NAME_LEN)?;
    let metadata = normalize_metadata(payload.metadata)?;
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
        insert into asset_folders (
          id,
          tenant_id,
          app_id,
          customer_id,
          parent_id,
          name,
          metadata_json
        )
        values ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(id)
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(payload.customer_id)
    .bind(payload.parent_id)
    .bind(name)
    .bind(metadata)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    let folder = find_folder(&state, &server_key, payload.customer_id, id).await?;

    Ok(Json(ApiResponse::ok(
        AssetFolderResponse { folder },
        request_id.to_string(),
    )))
}

pub async fn update_asset_folder(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(folder_id): Path<Uuid>,
    Json(payload): Json<UpdateAssetFolderRequest>,
) -> Result<Json<ApiResponse<AssetFolderResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    let current = find_folder_by_id(&state, &server_key, folder_id).await?;
    let name = payload
        .name
        .as_deref()
        .map(|value| normalize_name(value, "folder name", MAX_FOLDER_NAME_LEN))
        .transpose()?;
    let parent_id = payload.parent_id.flatten();
    if payload.parent_id.is_some() {
        ensure_folder_belongs(&state, &server_key, current.customer_id, parent_id).await?;
        ensure_folder_can_move(
            &state,
            &server_key,
            current.customer_id,
            folder_id,
            parent_id,
        )
        .await?;
    }
    let metadata = payload
        .metadata
        .map(|value| normalize_metadata(Some(value)))
        .transpose()?;

    sqlx::query(
        r#"
        update asset_folders
        set name = coalesce($4, name),
            parent_id = case when $5::bool then $6 else parent_id end,
            metadata_json = coalesce($7::jsonb, metadata_json),
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(folder_id)
    .bind(name)
    .bind(payload.parent_id.is_some())
    .bind(parent_id)
    .bind(metadata)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    let folder = find_folder_by_id(&state, &server_key, folder_id).await?;

    Ok(Json(ApiResponse::ok(
        AssetFolderResponse { folder },
        request_id.to_string(),
    )))
}

pub async fn delete_asset_folder(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(folder_id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeleteAssetFolderResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    let current = find_folder_by_id(&state, &server_key, folder_id).await?;
    ensure_folder_empty(&state, &server_key, current.customer_id, folder_id).await?;

    sqlx::query(
        r#"
        update asset_folders
        set deleted_at = now(),
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(folder_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeleteAssetFolderResponse {
            deleted: true,
            folder_id,
        },
        request_id.to_string(),
    )))
}

pub async fn create_asset_upload_url(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<CreateAssetUploadRequest>,
) -> Result<Json<ApiResponse<CreateAssetUploadResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    ensure_folder_belongs(&state, &server_key, payload.customer_id, payload.folder_id).await?;
    let file_name = normalize_file_name(&payload.file_name)?;
    let asset_type =
        normalize_asset_type_input(payload.asset_type.as_deref(), payload.kind.as_deref())?;
    let asset_role = normalize_upload_asset_role(payload.asset_role.as_deref())?;
    let mime_type = payload
        .mime_type
        .as_deref()
        .map(normalize_mime_type_required)
        .transpose()?;
    validate_asset_type_mime(&asset_type, mime_type.as_deref())?;
    let file_size = normalize_file_size(payload.file_size)?;
    let metadata = normalize_metadata(payload.metadata)?;
    let plain_token = generate_upload_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &plain_token)?;
    let token_prefix = display_prefix(&plain_token);
    let upload_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::minutes(UPLOAD_TOKEN_TTL_MINUTES);

    sqlx::query(
        r#"
        insert into asset_uploads (
          id,
          tenant_id,
          app_id,
          customer_id,
          folder_id,
          server_key_id,
          token_hash,
          token_prefix,
          file_name,
          asset_type,
          asset_role,
          mime_type,
          file_size,
          metadata_json,
          expires_at
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
        "#,
    )
    .bind(upload_id)
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(payload.customer_id)
    .bind(payload.folder_id)
    .bind(server_key.server_key_id)
    .bind(token_hash)
    .bind(&token_prefix)
    .bind(file_name)
    .bind(asset_type)
    .bind(asset_role)
    .bind(mime_type)
    .bind(file_size)
    .bind(metadata)
    .bind(expires_at)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    let upload_url = web_asset_path(
        &state,
        &format!("/api/server/web/v1/assets/uploads/{upload_id}"),
    );
    let upload = AssetUploadGrant {
        upload_id,
        method: "PUT",
        url: upload_url,
        upload_token: plain_token.clone(),
        token_prefix,
        expires_at,
        max_bytes: MAX_WEB_ASSET_UPLOAD_BYTES,
        headers: json!({
            "X-EntitleHub-Upload-Token": plain_token,
            "Content-Type": "application/octet-stream",
        }),
    };

    Ok(Json(ApiResponse::ok(
        CreateAssetUploadResponse { upload },
        request_id.to_string(),
    )))
}

pub async fn upload_customer_asset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(upload_id): Path<Uuid>,
    Query(query): Query<AssetUploadTokenQuery>,
    body: Bytes,
) -> Result<Json<ApiResponse<CustomerAssetResponse>>, AppError> {
    if body.is_empty() {
        return Err(AppError::validation_failed("asset file is required"));
    }
    if body.len() > MAX_WEB_ASSET_UPLOAD_BYTES {
        return Err(AppError::validation_failed("asset file is too large"));
    }
    let token = upload_token_from_request(&headers, query.token.as_deref())?;
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let upload = find_upload(&state, upload_id, &token_hash).await?;
    ensure_upload_is_usable(&upload)?;
    if upload
        .file_size
        .is_some_and(|expected| expected != body.len() as i64)
    {
        return Err(AppError::validation_failed(
            "asset file size does not match upload session",
        ));
    }
    let mime_type = upload
        .mime_type
        .clone()
        .or_else(|| content_type_from_headers(&headers))
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    validate_asset_type_mime(&upload.asset_type, Some(&mime_type))?;

    let asset_id = Uuid::new_v4();
    let extension = extension_for_mime(&mime_type);
    let storage_key = format!(
        "tenants/{}/web-assets/{}/{}.{}",
        upload.tenant_id, upload.customer_id, upload_id, extension
    );
    state.object_store.put_bytes(&storage_key, &body).await?;
    let checksum = format!("{:x}", Sha256::digest(&body));
    let public_url = asset_download_url(&state, asset_id);
    let consumed = consume_upload(&state, upload_id, &token_hash).await?;
    let asset = insert_customer_asset(
        &state,
        NewCustomerAsset {
            id: asset_id,
            tenant_id: consumed.tenant_id,
            app_id: consumed.app_id,
            customer_id: consumed.customer_id,
            folder_id: consumed.folder_id,
            ai_asset_id: None,
            name: consumed.file_name,
            asset_type: consumed.asset_type,
            asset_role: consumed.asset_role,
            source: "user_upload".to_owned(),
            storage_key: Some(storage_key),
            public_url: Some(public_url),
            mime_type: Some(mime_type),
            file_size: Some(body.len() as i64),
            checksum_sha256: Some(checksum),
            metadata_json: consumed.metadata_json,
            server_key_id: Some(consumed.server_key_id),
        },
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        CustomerAssetResponse::from_asset(asset),
        request_id.to_string(),
    )))
}

pub async fn direct_upload_customer_asset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<DirectAssetUploadQuery>,
    body: Bytes,
) -> Result<Json<ApiResponse<CustomerAssetResponse>>, AppError> {
    if body.is_empty() {
        return Err(AppError::validation_failed("asset file is required"));
    }
    if body.len() > MAX_WEB_ASSET_UPLOAD_BYTES {
        return Err(AppError::validation_failed("asset file is too large"));
    }
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, query.customer_id).await?;
    ensure_folder_belongs(&state, &server_key, query.customer_id, query.folder_id).await?;
    let file_name = normalize_file_name(&query.file_name)?;
    let asset_type =
        normalize_asset_type_input(query.asset_type.as_deref(), query.kind.as_deref())?;
    let asset_role = normalize_upload_asset_role(query.asset_role.as_deref())?;
    let mime_type = query
        .mime_type
        .as_deref()
        .and_then(normalize_mime_type)
        .or_else(|| content_type_from_headers(&headers))
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    validate_asset_type_mime(&asset_type, Some(&mime_type))?;

    let asset_id = Uuid::new_v4();
    let extension = extension_for_mime(&mime_type);
    let storage_key = format!(
        "tenants/{}/web-assets/{}/{}.{}",
        server_key.tenant_id, query.customer_id, asset_id, extension
    );
    state.object_store.put_bytes(&storage_key, &body).await?;
    let checksum = format!("{:x}", Sha256::digest(&body));
    let public_url = asset_download_url(&state, asset_id);
    let asset = insert_customer_asset(
        &state,
        NewCustomerAsset {
            id: asset_id,
            tenant_id: server_key.tenant_id,
            app_id: server_key.app_id,
            customer_id: query.customer_id,
            folder_id: query.folder_id,
            ai_asset_id: None,
            name: file_name,
            asset_type,
            asset_role,
            source: "user_upload".to_owned(),
            storage_key: Some(storage_key),
            public_url: Some(public_url),
            mime_type: Some(mime_type),
            file_size: Some(body.len() as i64),
            checksum_sha256: Some(checksum),
            metadata_json: json!({}),
            server_key_id: Some(server_key.server_key_id),
        },
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        CustomerAssetResponse::from_asset(asset),
        request_id.to_string(),
    )))
}

pub async fn list_customer_assets(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<CustomerAssetListQuery>,
) -> Result<Json<ApiResponse<CustomerAssetListResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, query.customer_id).await?;
    let (filter_folder, folder_id) =
        normalize_optional_uuid_filter(query.folder_id.as_deref(), "folder_id")?;
    let asset_type =
        normalize_optional_asset_type_filter(query.asset_type.as_deref(), query.kind.as_deref())?;
    let asset_role = query
        .asset_role
        .as_deref()
        .map(normalize_asset_role)
        .transpose()?;
    let source = query.source.as_deref().map(normalize_source).transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_assets(
        &state,
        &server_key,
        query.customer_id,
        filter_folder,
        folder_id,
        asset_type.as_deref(),
        asset_role.as_deref(),
        source.as_deref(),
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        CustomerAssetListResponse {
            items: items.into_iter().map(CustomerAssetView::from).collect(),
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

pub async fn get_customer_asset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<ApiResponse<CustomerAssetResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    let asset = find_asset(&state, &server_key, asset_id).await?;

    Ok(Json(ApiResponse::ok(
        CustomerAssetResponse::from_asset(asset),
        request_id.to_string(),
    )))
}

pub async fn update_customer_asset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(asset_id): Path<Uuid>,
    Json(payload): Json<UpdateCustomerAssetRequest>,
) -> Result<Json<ApiResponse<CustomerAssetResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    let current = find_asset(&state, &server_key, asset_id).await?;
    let name = payload
        .name
        .as_deref()
        .map(|value| normalize_name(value, "asset name", MAX_NAME_LEN))
        .transpose()?;
    let folder_id = payload.folder_id.flatten();
    if payload.folder_id.is_some() {
        ensure_folder_belongs(&state, &server_key, current.customer_id, folder_id).await?;
    }
    let asset_role = payload
        .asset_role
        .as_deref()
        .map(normalize_asset_role)
        .transpose()?;
    let metadata = payload
        .metadata
        .map(|value| normalize_metadata(Some(value)))
        .transpose()?;

    sqlx::query(
        r#"
        update customer_assets
        set name = coalesce($4, name),
            folder_id = case when $5::bool then $6 else folder_id end,
            asset_role = coalesce($7, asset_role),
            metadata_json = coalesce($8::jsonb, metadata_json),
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(asset_id)
    .bind(name)
    .bind(payload.folder_id.is_some())
    .bind(folder_id)
    .bind(asset_role)
    .bind(metadata)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    let asset = find_asset(&state, &server_key, asset_id).await?;

    Ok(Json(ApiResponse::ok(
        CustomerAssetResponse::from_asset(asset),
        request_id.to_string(),
    )))
}

pub async fn delete_customer_asset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeleteCustomerAssetResponse>>, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    find_asset(&state, &server_key, asset_id).await?;

    sqlx::query(
        r#"
        update customer_assets
        set status = 'deleted',
            deleted_at = now(),
            deleted_by_server_key_id = $4,
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(asset_id)
    .bind(server_key.server_key_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeleteCustomerAssetResponse {
            deleted: true,
            asset_id,
        },
        request_id.to_string(),
    )))
}

pub async fn download_customer_asset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(asset_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let server_key = authenticate_web_assets_key(&state, &headers).await?;
    let asset = sqlx::query_as::<_, AssetStorageRecord>(
        r#"
        select
          storage_key,
          mime_type,
          file_size
        from customer_assets
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
          and status = 'ready'
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(asset_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("asset not found"))?;
    let storage_key = asset
        .storage_key
        .ok_or_else(|| AppError::not_found("asset file not found"))?;
    let stored = state.object_store.open(&storage_key).await?;
    let mut reader = stored.reader;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| AppError::dependency(format!("asset read failed: {error}")))?;
    let content_type = asset
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(
            "content-length",
            asset.file_size.unwrap_or(stored.size as i64).to_string(),
        )
        .header("cache-control", "private, max-age=300")
        .body(Body::from(bytes))
        .map_err(|error| AppError::dependency(format!("asset response failed: {error}")))
}

pub async fn mirror_generated_ai_asset(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
    customer_id: Uuid,
    ai_asset_id: Uuid,
    asset_type: &str,
    public_url: &str,
    storage_key: &str,
    mime_type: &str,
    file_size: i64,
    checksum_sha256: &str,
    metadata: Value,
) -> Result<Uuid, AppError> {
    let extension = extension_for_mime(mime_type);
    let name = format!("generated-{ai_asset_id}.{extension}");

    sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into customer_assets (
          id,
          tenant_id,
          app_id,
          customer_id,
          ai_asset_id,
          name,
          asset_type,
          asset_role,
          source,
          status,
          storage_key,
          public_url,
          mime_type,
          file_size,
          checksum_sha256,
          metadata_json
        )
        values (
          gen_random_uuid(), $1, $2, $3, $4, $5, $6, 'generated',
          'generated', 'ready', $7, $8, $9, $10, $11, $12
        )
        on conflict (ai_asset_id) where ai_asset_id is not null do update
        set updated_at = customer_assets.updated_at
        returning id
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(customer_id)
    .bind(ai_asset_id)
    .bind(name)
    .bind(asset_type)
    .bind(storage_key)
    .bind(public_url)
    .bind(mime_type)
    .bind(file_size)
    .bind(checksum_sha256)
    .bind(metadata)
    .fetch_one(&state.db)
    .await
    .map_err(map_db_error)
}

pub async fn find_generation_asset(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
    customer_id: Uuid,
    asset_id: Uuid,
) -> Result<GenerationAssetRecord, AppError> {
    sqlx::query_as::<_, GenerationAssetRecord>(
        r#"
        select
          id,
          customer_id,
          asset_type,
          asset_role,
          public_url,
          mime_type,
          file_size
        from customer_assets
        where tenant_id = $1
          and app_id = $2
          and customer_id = $3
          and id = $4
          and deleted_at is null
          and status = 'ready'
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(customer_id)
    .bind(asset_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("asset not found"))
}

#[derive(Debug)]
struct NewCustomerAsset {
    id: Uuid,
    tenant_id: Uuid,
    app_id: Uuid,
    customer_id: Uuid,
    folder_id: Option<Uuid>,
    ai_asset_id: Option<Uuid>,
    name: String,
    asset_type: String,
    asset_role: String,
    source: String,
    storage_key: Option<String>,
    public_url: Option<String>,
    mime_type: Option<String>,
    file_size: Option<i64>,
    checksum_sha256: Option<String>,
    metadata_json: Value,
    server_key_id: Option<Uuid>,
}

async fn insert_customer_asset(
    state: &AppState,
    input: NewCustomerAsset,
) -> Result<CustomerAsset, AppError> {
    sqlx::query(
        r#"
        insert into customer_assets (
          id,
          tenant_id,
          app_id,
          customer_id,
          folder_id,
          ai_asset_id,
          name,
          asset_type,
          asset_role,
          source,
          status,
          storage_key,
          public_url,
          mime_type,
          file_size,
          checksum_sha256,
          metadata_json,
          created_by_server_key_id
        )
        values (
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'ready',
          $11, $12, $13, $14, $15, $16, $17
        )
        "#,
    )
    .bind(input.id)
    .bind(input.tenant_id)
    .bind(input.app_id)
    .bind(input.customer_id)
    .bind(input.folder_id)
    .bind(input.ai_asset_id)
    .bind(input.name)
    .bind(input.asset_type)
    .bind(input.asset_role)
    .bind(input.source)
    .bind(input.storage_key)
    .bind(input.public_url)
    .bind(input.mime_type)
    .bind(input.file_size)
    .bind(input.checksum_sha256)
    .bind(input.metadata_json)
    .bind(input.server_key_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    find_asset_by_id(state, input.tenant_id, input.app_id, input.id).await
}

async fn list_folders(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    filter_parent: bool,
    parent_id: Option<Uuid>,
    page: i64,
    page_size: i64,
) -> Result<Vec<AssetFolder>, AppError> {
    let offset = (page - 1) * page_size;

    sqlx::query_as::<_, AssetFolder>(
        r#"
        select
          id,
          customer_id,
          parent_id,
          name,
          metadata_json,
          created_at,
          updated_at
        from asset_folders
        where tenant_id = $1
          and app_id = $2
          and customer_id = $3
          and deleted_at is null
          and (
            not $4::bool
            or ($5::uuid is null and parent_id is null)
            or ($5::uuid is not null and parent_id = $5)
          )
        order by created_at desc, id desc
        limit $6 offset $7
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(filter_parent)
    .bind(parent_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn list_assets(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    filter_folder: bool,
    folder_id: Option<Uuid>,
    asset_type: Option<&str>,
    asset_role: Option<&str>,
    source: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<CustomerAsset>, AppError> {
    let offset = (page - 1) * page_size;

    sqlx::query_as::<_, CustomerAsset>(
        r#"
        select
          id,
          customer_id,
          folder_id,
          ai_asset_id,
          name,
          asset_type,
          asset_role,
          source,
          status,
          public_url,
          mime_type,
          file_size,
          checksum_sha256,
          metadata_json,
          created_at,
          updated_at
        from customer_assets
        where tenant_id = $1
          and app_id = $2
          and customer_id = $3
          and deleted_at is null
          and status = 'ready'
          and (
            not $4::bool
            or ($5::uuid is null and folder_id is null)
            or ($5::uuid is not null and folder_id = $5)
          )
          and ($6::text is null or asset_type = $6)
          and ($7::text is null or asset_role = $7)
          and ($8::text is null or source = $8)
        order by created_at desc, id desc
        limit $9 offset $10
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(filter_folder)
    .bind(folder_id)
    .bind(asset_type)
    .bind(asset_role)
    .bind(source)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_folder(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    folder_id: Uuid,
) -> Result<AssetFolder, AppError> {
    sqlx::query_as::<_, AssetFolder>(
        r#"
        select
          id,
          customer_id,
          parent_id,
          name,
          metadata_json,
          created_at,
          updated_at
        from asset_folders
        where tenant_id = $1
          and app_id = $2
          and customer_id = $3
          and id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(folder_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("asset folder not found"))
}

async fn find_folder_by_id(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    folder_id: Uuid,
) -> Result<AssetFolder, AppError> {
    sqlx::query_as::<_, AssetFolder>(
        r#"
        select
          id,
          customer_id,
          parent_id,
          name,
          metadata_json,
          created_at,
          updated_at
        from asset_folders
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(folder_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("asset folder not found"))
}

async fn find_asset(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    asset_id: Uuid,
) -> Result<CustomerAsset, AppError> {
    find_asset_by_id(state, server_key.tenant_id, server_key.app_id, asset_id).await
}

async fn find_asset_by_id(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
    asset_id: Uuid,
) -> Result<CustomerAsset, AppError> {
    sqlx::query_as::<_, CustomerAsset>(
        r#"
        select
          id,
          customer_id,
          folder_id,
          ai_asset_id,
          name,
          asset_type,
          asset_role,
          source,
          status,
          public_url,
          mime_type,
          file_size,
          checksum_sha256,
          metadata_json,
          created_at,
          updated_at
        from customer_assets
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and deleted_at is null
          and status = 'ready'
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(asset_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("asset not found"))
}

async fn find_upload(
    state: &AppState,
    upload_id: Uuid,
    token_hash: &str,
) -> Result<AssetUploadRecord, AppError> {
    sqlx::query_as::<_, AssetUploadRecord>(
        r#"
        select
          id,
          tenant_id,
          app_id,
          customer_id,
          folder_id,
          server_key_id,
          file_name,
          asset_type,
          asset_role,
          mime_type,
          file_size,
          metadata_json,
          expires_at,
          consumed_at
        from asset_uploads
        where id = $1
          and token_hash = $2
        "#,
    )
    .bind(upload_id)
    .bind(token_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::token_invalid("asset upload token invalid"))
}

async fn consume_upload(
    state: &AppState,
    upload_id: Uuid,
    token_hash: &str,
) -> Result<AssetUploadRecord, AppError> {
    sqlx::query_as::<_, AssetUploadRecord>(
        r#"
        update asset_uploads
        set consumed_at = now()
        where id = $1
          and token_hash = $2
          and consumed_at is null
          and expires_at > now()
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          folder_id,
          server_key_id,
          file_name,
          asset_type,
          asset_role,
          mime_type,
          file_size,
          metadata_json,
          expires_at,
          consumed_at
        "#,
    )
    .bind(upload_id)
    .bind(token_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::conflict("asset upload session already consumed or expired"))
}

async fn authenticate_web_assets_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ServerApiKeyContext, AppError> {
    authenticate_server_key(state, headers, ai_invoke_scope()).await
}

async fn ensure_customer_active(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    let customer = CustomerRepository::new(state.db.clone())
        .find_by_id(tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    if customer.status != "active" {
        return Err(AppError::account_disabled());
    }

    Ok(())
}

async fn ensure_folder_belongs(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    folder_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(folder_id) = folder_id else {
        return Ok(());
    };
    find_folder(state, server_key, customer_id, folder_id)
        .await
        .map(|_| ())
}

async fn ensure_folder_can_move(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    folder_id: Uuid,
    parent_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    if parent_id == folder_id {
        return Err(AppError::business_rule_failed(
            "asset folder cannot be moved into itself",
        ));
    }
    let is_descendant = sqlx::query_scalar::<_, bool>(
        r#"
        with recursive descendants as (
          select id
          from asset_folders
          where tenant_id = $1
            and app_id = $2
            and customer_id = $3
            and parent_id = $4
            and deleted_at is null
          union all
          select f.id
          from asset_folders f
          join descendants d on f.parent_id = d.id
          where f.tenant_id = $1
            and f.app_id = $2
            and f.customer_id = $3
            and f.deleted_at is null
        )
        select exists(select 1 from descendants where id = $5)
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(folder_id)
    .bind(parent_id)
    .fetch_one(&state.db)
    .await
    .map_err(map_db_error)?;
    if is_descendant {
        return Err(AppError::business_rule_failed(
            "asset folder cannot be moved into a child folder",
        ));
    }

    Ok(())
}

async fn ensure_folder_empty(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    folder_id: Uuid,
) -> Result<(), AppError> {
    let child_count = sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from asset_folders
        where tenant_id = $1
          and app_id = $2
          and customer_id = $3
          and parent_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(folder_id)
    .fetch_one(&state.db)
    .await
    .map_err(map_db_error)?;
    if child_count > 0 {
        return Err(AppError::business_rule_failed(
            "asset folder must be empty before deletion",
        ));
    }
    let asset_count = sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from customer_assets
        where tenant_id = $1
          and app_id = $2
          and customer_id = $3
          and folder_id = $4
          and deleted_at is null
          and status = 'ready'
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(folder_id)
    .fetch_one(&state.db)
    .await
    .map_err(map_db_error)?;
    if asset_count > 0 {
        return Err(AppError::business_rule_failed(
            "asset folder must be empty before deletion",
        ));
    }

    Ok(())
}

fn normalize_name(value: &str, field: &str, max_len: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > max_len
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(AppError::validation_failed(format!("{field} is invalid")));
    }

    Ok(value.to_owned())
}

fn normalize_file_name(value: &str) -> Result<String, AppError> {
    let value = normalize_name(value, "file name", MAX_NAME_LEN)?;
    if value.contains('/') || value.contains('\\') {
        return Err(AppError::validation_failed("file name is invalid"));
    }

    Ok(value)
}

fn normalize_asset_type(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "image" | "video" | "audio" | "file" => Ok(value),
        _ => Err(AppError::validation_failed("asset type is invalid")),
    }
}

fn normalize_asset_type_input(
    asset_type: Option<&str>,
    kind: Option<&str>,
) -> Result<String, AppError> {
    let value = asset_type
        .or(kind)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::validation_failed("asset type or kind is required"))?;
    normalize_asset_type(value)
}

fn normalize_optional_asset_type_filter(
    asset_type: Option<&str>,
    kind: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(value) = asset_type
        .or(kind)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    normalize_asset_type(value).map(Some)
}

fn normalize_upload_asset_role(value: Option<&str>) -> Result<String, AppError> {
    match value {
        Some(value) => {
            let value = normalize_asset_role(value)?;
            if value == "generated" {
                return Err(AppError::validation_failed("asset role is invalid"));
            }
            Ok(value)
        }
        None => Ok("upload".to_owned()),
    }
}

fn normalize_asset_role(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "upload" | "generated" | "reference" | "first_frame" | "last_frame" | "brand" | "other" => {
            Ok(value)
        }
        _ => Err(AppError::validation_failed("asset role is invalid")),
    }
}

fn normalize_source(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "upload" | "user_upload" => Ok("user_upload".to_owned()),
        "ai" | "generated" => Ok("generated".to_owned()),
        "digital-human" | "digital_human" | "product" | "import" | "imported" => {
            Ok("imported".to_owned())
        }
        _ => Err(AppError::validation_failed("asset source is invalid")),
    }
}

fn public_source_alias(source: &str) -> &str {
    match source {
        "user_upload" => "upload",
        "generated" => "ai",
        "imported" => "imported",
        value => value,
    }
}

fn asset_thumbnail_url(asset: &CustomerAsset) -> Option<String> {
    if asset.asset_type == "image" {
        return asset.public_url.clone();
    }
    first_metadata_string(
        &asset.metadata,
        &[
            "thumbnailUrl",
            "thumbnail_url",
            "coverUrl",
            "cover_url",
            "posterUrl",
            "poster_url",
        ],
    )
}

fn asset_duration_seconds(metadata: &Value) -> Option<i64> {
    for key in ["duration", "durationSeconds", "duration_seconds", "seconds"] {
        if let Some(value) = metadata.get(key).and_then(value_to_positive_seconds) {
            return Some(value);
        }
    }
    None
}

fn first_metadata_string(metadata: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| metadata.get(*key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn value_to_positive_seconds(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return (value > 0).then_some(value);
    }
    if let Some(value) = value.as_f64() {
        return (value.is_finite() && value > 0.0).then_some(value.ceil() as i64);
    }
    value
        .as_str()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.ceil() as i64)
}

fn normalize_metadata(value: Option<Value>) -> Result<Value, AppError> {
    let value = value.unwrap_or_else(|| json!({}));
    if !value.is_object() {
        return Err(AppError::validation_failed(
            "asset metadata must be an object",
        ));
    }

    Ok(value)
}

fn normalize_file_size(value: Option<i64>) -> Result<Option<i64>, AppError> {
    match value {
        Some(value) if value <= 0 || value as usize > MAX_WEB_ASSET_UPLOAD_BYTES => {
            Err(AppError::validation_failed("asset file size is invalid"))
        }
        value => Ok(value),
    }
}

fn normalize_optional_uuid_filter(
    value: Option<&str>,
    field: &str,
) -> Result<(bool, Option<Uuid>), AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((false, None));
    };
    if matches!(value, "root" | "null" | "none") {
        return Ok((true, None));
    }
    let id = Uuid::parse_str(value)
        .map_err(|_| AppError::validation_failed(format!("{field} is invalid")))?;

    Ok((true, Some(id)))
}

fn normalize_mime_type_required(value: &str) -> Result<String, AppError> {
    normalize_mime_type(value).ok_or_else(|| AppError::validation_failed("mime type is invalid"))
}

fn normalize_mime_type(value: &str) -> Option<String> {
    let value = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 120
        || value.contains('\0')
        || value.chars().any(char::is_control)
        || !value.contains('/')
    {
        None
    } else {
        Some(value)
    }
}

fn validate_asset_type_mime(asset_type: &str, mime_type: Option<&str>) -> Result<(), AppError> {
    let Some(mime_type) = mime_type else {
        return Ok(());
    };
    let valid = match asset_type {
        "image" => mime_type.starts_with("image/"),
        "video" => mime_type.starts_with("video/"),
        "audio" => mime_type.starts_with("audio/"),
        "file" => true,
        _ => false,
    };
    if !valid {
        return Err(AppError::validation_failed(
            "mime type does not match asset type",
        ));
    }

    Ok(())
}

fn content_type_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_mime_type)
}

fn upload_token_from_request(
    headers: &HeaderMap,
    query_token: Option<&str>,
) -> Result<String, AppError> {
    let token = headers
        .get(UPLOAD_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .or(query_token)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(AppError::unauthenticated)?;
    if !token.starts_with(UPLOAD_TOKEN_PREFIX)
        || token.contains('\0')
        || token.chars().any(char::is_control)
    {
        return Err(AppError::token_invalid("asset upload token invalid"));
    }

    Ok(token.to_owned())
}

fn ensure_upload_is_usable(upload: &AssetUploadRecord) -> Result<(), AppError> {
    if upload.consumed_at.is_some() {
        return Err(AppError::conflict("asset upload session already consumed"));
    }
    if upload.expires_at <= Utc::now() {
        return Err(AppError::token_expired());
    }

    Ok(())
}

fn generate_upload_token() -> String {
    format!("{UPLOAD_TOKEN_PREFIX}{}", generate_token())
}

fn display_prefix(token: &str) -> String {
    token.chars().take(UPLOAD_TOKEN_DISPLAY_LEN).collect()
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
}

fn web_asset_path(state: &AppState, path: &str) -> String {
    match state.config.app.base_url.as_deref() {
        Some(base_url) => format!("{}{}", base_url.trim_end_matches('/'), path),
        None => path.to_owned(),
    }
}

fn asset_download_url(state: &AppState, asset_id: Uuid) -> String {
    web_asset_path(
        state,
        &format!("/api/server/web/v1/assets/{asset_id}/download"),
    )
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/mpeg" => "mpeg",
        "video/x-msvideo" => "avi",
        "video/x-matroska" => "mkv",
        "audio/mpeg" => "mp3",
        "audio/wav" | "audio/x-wav" => "wav",
        "audio/mp4" => "m4a",
        "audio/webm" => "weba",
        _ => "bin",
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    tracing::error!(%error, "web asset database error");
    AppError::dependency(format!("web asset database error: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        asset_duration_seconds, asset_thumbnail_url, normalize_asset_role, normalize_asset_type,
        normalize_asset_type_input, normalize_file_name, normalize_file_size, normalize_metadata,
        normalize_mime_type, normalize_optional_asset_type_filter, normalize_optional_uuid_filter,
        normalize_source, validate_asset_type_mime, CustomerAsset, MAX_WEB_ASSET_UPLOAD_BYTES,
    };

    #[test]
    fn asset_filters_and_names_are_validated() {
        assert_eq!(normalize_asset_type(" Image ").expect("type"), "image");
        assert_eq!(
            normalize_asset_role("First_Frame").expect("role"),
            "first_frame"
        );
        assert_eq!(normalize_file_name("cover.png").expect("name"), "cover.png");
        assert!(normalize_file_name("../cover.png").is_err());
        assert!(normalize_asset_type("folder").is_err());
    }

    #[test]
    fn uuid_filter_supports_root_and_uuid() {
        assert_eq!(
            normalize_optional_uuid_filter(None, "folder").unwrap(),
            (false, None)
        );
        assert_eq!(
            normalize_optional_uuid_filter(Some("root"), "folder").unwrap(),
            (true, None)
        );
        assert!(normalize_optional_uuid_filter(Some("bad"), "folder").is_err());
    }

    #[test]
    fn metadata_and_file_size_are_bounded() {
        assert!(normalize_metadata(Some(json!({ "a": 1 }))).is_ok());
        assert!(normalize_metadata(Some(json!([]))).is_err());
        assert!(normalize_file_size(Some(0)).is_err());
        assert!(normalize_file_size(Some((MAX_WEB_ASSET_UPLOAD_BYTES + 1) as i64)).is_err());
    }

    #[test]
    fn mime_type_must_match_declared_asset_type() {
        assert_eq!(
            normalize_mime_type(" Image/PNG; charset=utf-8 ").as_deref(),
            Some("image/png")
        );
        assert!(validate_asset_type_mime("image", Some("image/png")).is_ok());
        assert!(validate_asset_type_mime("image", Some("video/mp4")).is_err());
        assert!(validate_asset_type_mime("file", Some("video/mp4")).is_ok());
    }

    #[test]
    fn asset_kind_and_source_aliases_are_compatible() {
        assert_eq!(
            normalize_asset_type_input(None, Some(" Video ")).expect("kind"),
            "video"
        );
        assert_eq!(
            normalize_optional_asset_type_filter(None, Some("image")).expect("filter"),
            Some("image".to_owned())
        );
        assert_eq!(normalize_source("upload").expect("source"), "user_upload");
        assert_eq!(normalize_source("ai").expect("source"), "generated");
        assert_eq!(
            normalize_source("digital-human").expect("source"),
            "imported"
        );
    }

    #[test]
    fn asset_view_metadata_exposes_thumbnail_and_duration() {
        let image = CustomerAsset {
            id: uuid::Uuid::new_v4(),
            customer_id: uuid::Uuid::new_v4(),
            folder_id: None,
            ai_asset_id: None,
            name: "cover.png".to_owned(),
            asset_type: "image".to_owned(),
            asset_role: "upload".to_owned(),
            source: "user_upload".to_owned(),
            status: "ready".to_owned(),
            public_url: Some("https://cdn.example.com/cover.png".to_owned()),
            mime_type: Some("image/png".to_owned()),
            file_size: Some(12),
            checksum_sha256: None,
            metadata: json!({}),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(
            asset_thumbnail_url(&image).as_deref(),
            Some("https://cdn.example.com/cover.png")
        );
        assert_eq!(
            asset_duration_seconds(&json!({"duration_seconds": 8.2})),
            Some(9)
        );
    }
}
