use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        customer::repository::CustomerRepository,
        server_api::{ai_invoke_scope, authenticate_server_key, ServerApiKeyContext},
    },
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;
const MAX_TITLE_LEN: usize = 160;
const MAX_DESCRIPTION_LEN: usize = 2_000;
const MAX_TAG_LEN: usize = 40;
const MAX_TAGS: usize = 20;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct WebWork {
    pub id: Uuid,
    pub owner_customer_id: Uuid,
    pub source_job_id: Option<Uuid>,
    pub title: String,
    pub description: Option<String>,
    pub work_type: String,
    pub status: String,
    pub visibility: String,
    pub cover_asset_id: Option<Uuid>,
    pub primary_asset_id: Uuid,
    pub primary_asset_url: Option<String>,
    pub primary_asset_mime_type: Option<String>,
    pub primary_asset_file_size: Option<i64>,
    pub cover_asset_url: Option<String>,
    #[sqlx(rename = "metadata_json")]
    pub metadata: Value,
    pub favorite_count: i64,
    pub favorited: bool,
    #[serde(rename = "sourceMode")]
    pub source_mode: Option<String>,
    #[serde(rename = "referenceCount")]
    pub reference_count: i64,
    #[serde(rename = "hasFirstFrame")]
    pub has_first_frame: bool,
    #[serde(rename = "hasLastFrame")]
    pub has_last_frame: bool,
    pub publication_status: Option<String>,
    #[serde(rename = "publishedAt")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(rename = "favoritedAt")]
    pub favorited_at: Option<DateTime<Utc>>,
    #[serde(rename = "downloadedAt")]
    pub downloaded_at: Option<DateTime<Utc>>,
    pub publication_tags: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct WebWorkListResponse {
    pub items: Vec<WebWork>,
    pub works: Vec<WebWork>,
    pub meta: ListMeta,
    pub pagination: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct WebWorkResponse {
    pub work: WebWork,
}

#[derive(Debug, Serialize)]
pub struct WebWorkFavoriteResponse {
    pub work_id: Uuid,
    pub customer_id: Uuid,
    pub favorited: bool,
    #[serde(rename = "favoritedAt")]
    pub favorited_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct WebWorkPublicationResponse {
    pub work: WebWork,
}

#[derive(Debug, Serialize)]
pub struct WebWorkDownloadResponse {
    pub work: WebWork,
    #[serde(rename = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(rename = "downloadedAt")]
    pub downloaded_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct DeleteWebWorkResponse {
    pub deleted: bool,
    pub work_id: Uuid,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
    #[serde(rename = "pageSize")]
    pub page_size_alias: i64,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

impl ListMeta {
    fn new(page: i64, page_size: i64, has_more: bool) -> Self {
        Self {
            page,
            page_size,
            page_size_alias: page_size,
            has_more,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WebWorkListQuery {
    pub customer_id: Uuid,
    #[serde(rename = "type")]
    pub work_type: Option<String>,
    pub visibility: Option<String>,
    pub favorite: Option<bool>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WebWorkCustomerQuery {
    pub customer_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct WebGalleryQuery {
    #[serde(rename = "type")]
    pub work_type: Option<String>,
    pub customer_id: Option<Uuid>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebWorkRequest {
    pub customer_id: Uuid,
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub cover_asset_id: Option<Option<Uuid>>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct WebWorkCustomerRequest {
    pub customer_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct PublishWebWorkRequest {
    pub customer_id: Uuid,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, FromRow)]
struct WorkAssetOwner {
    customer_id: Uuid,
}

pub async fn list_works(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<WebWorkListQuery>,
) -> Result<Json<ApiResponse<WebWorkListResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, query.customer_id).await?;
    let work_type = query
        .work_type
        .as_deref()
        .map(normalize_work_type)
        .transpose()?;
    let visibility = query
        .visibility
        .as_deref()
        .map(normalize_visibility)
        .transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let fetch_limit = page_size + 1;
    let items = if query.favorite.unwrap_or(false) {
        list_favorite_works(
            &state,
            &server_key,
            query.customer_id,
            work_type.as_deref(),
            visibility.as_deref(),
            page,
            fetch_limit,
        )
        .await?
    } else {
        list_owner_works(
            &state,
            &server_key,
            query.customer_id,
            work_type.as_deref(),
            visibility.as_deref(),
            page,
            fetch_limit,
        )
        .await?
    };

    let has_more = items.len() as i64 > page_size;
    let items = items
        .into_iter()
        .take(page_size as usize)
        .collect::<Vec<_>>();
    let meta = ListMeta::new(page, page_size, has_more);
    Ok(Json(ApiResponse::ok(
        WebWorkListResponse {
            works: items.clone(),
            items,
            meta: meta.clone(),
            pagination: meta,
        },
        request_id.to_string(),
    )))
}

pub async fn get_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Query(query): Query<WebWorkCustomerQuery>,
) -> Result<Json<ApiResponse<WebWorkResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, query.customer_id).await?;
    let work = find_visible_work(&state, &server_key, work_id, query.customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebWorkResponse { work },
        request_id.to_string(),
    )))
}

pub async fn update_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<UpdateWebWorkRequest>,
) -> Result<Json<ApiResponse<WebWorkResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;
    let title = payload.title.as_deref().map(normalize_title).transpose()?;
    let description = payload
        .description
        .clone()
        .map(normalize_optional_description)
        .transpose()?
        .flatten();
    let cover_asset_id = payload.cover_asset_id.flatten();
    if payload.cover_asset_id.is_some() {
        ensure_asset_belongs_to_customer(&state, &server_key, payload.customer_id, cover_asset_id)
            .await?;
    }
    let metadata = payload
        .metadata
        .map(|value| normalize_metadata(Some(value)))
        .transpose()?;

    sqlx::query(
        r#"
        update customer_works
        set title = coalesce($5, title),
            description = case when $6::bool then $7 else description end,
            cover_asset_id = case when $8::bool then $9 else cover_asset_id end,
            metadata_json = coalesce($10::jsonb, metadata_json),
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and owner_customer_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .bind(title)
    .bind(payload.description.is_some())
    .bind(description)
    .bind(payload.cover_asset_id.is_some())
    .bind(cover_asset_id)
    .bind(metadata)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    let work = find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebWorkResponse { work },
        request_id.to_string(),
    )))
}

pub async fn delete_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<WebWorkCustomerRequest>,
) -> Result<Json<ApiResponse<DeleteWebWorkResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;

    sqlx::query(
        r#"
        update customer_works
        set status = 'deleted',
            visibility = 'private',
            deleted_at = now(),
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and owner_customer_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    sqlx::query(
        r#"
        update work_publications
        set status = 'unpublished',
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and work_id = $3
          and status = 'published'
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeleteWebWorkResponse {
            deleted: true,
            work_id,
        },
        request_id.to_string(),
    )))
}

pub async fn favorite_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<WebWorkCustomerRequest>,
) -> Result<Json<ApiResponse<WebWorkFavoriteResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    find_visible_work(&state, &server_key, work_id, payload.customer_id).await?;

    let favorited_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        insert into work_favorites (
          tenant_id,
          app_id,
          work_id,
          customer_id
        )
        values ($1, $2, $3, $4)
        on conflict (tenant_id, app_id, work_id, customer_id)
          where deleted_at is null do nothing
        returning created_at
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?;
    let favorited_at = match favorited_at {
        Some(value) => Some(value),
        None => active_favorite_at(&state, &server_key, work_id, payload.customer_id).await?,
    };

    Ok(Json(ApiResponse::ok(
        WebWorkFavoriteResponse {
            work_id,
            customer_id: payload.customer_id,
            favorited: true,
            favorited_at,
        },
        request_id.to_string(),
    )))
}

pub async fn unfavorite_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<WebWorkCustomerRequest>,
) -> Result<Json<ApiResponse<WebWorkFavoriteResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;

    sqlx::query(
        r#"
        update work_favorites
        set deleted_at = now()
        where tenant_id = $1
          and app_id = $2
          and work_id = $3
          and customer_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        WebWorkFavoriteResponse {
            work_id,
            customer_id: payload.customer_id,
            favorited: false,
            favorited_at: None,
        },
        request_id.to_string(),
    )))
}

pub async fn download_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<WebWorkCustomerRequest>,
) -> Result<Json<ApiResponse<WebWorkDownloadResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    let visible = find_visible_work(&state, &server_key, work_id, payload.customer_id).await?;
    let downloaded_at = sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        insert into work_downloads (
          tenant_id,
          app_id,
          work_id,
          customer_id
        )
        values ($1, $2, $3, $4)
        on conflict (tenant_id, app_id, work_id, customer_id) do update
        set download_count = work_downloads.download_count + 1,
            downloaded_at = now(),
            updated_at = now()
        returning downloaded_at
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .fetch_one(&state.db)
    .await
    .map_err(map_db_error)?;
    let work = find_visible_work(&state, &server_key, work_id, payload.customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebWorkDownloadResponse {
            download_url: visible.primary_asset_url,
            downloaded_at: Some(downloaded_at),
            work,
        },
        request_id.to_string(),
    )))
}

pub async fn publish_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<PublishWebWorkRequest>,
) -> Result<Json<ApiResponse<WebWorkPublicationResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;
    let tags = normalize_tags(payload.tags)?;

    sqlx::query(
        r#"
        update customer_works
        set visibility = 'public',
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and owner_customer_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    sqlx::query(
        r#"
        insert into work_publications (
          tenant_id,
          app_id,
          work_id,
          status,
          published_at,
          tags,
          sort_score
        )
        values ($1, $2, $3, 'published', now(), $4, extract(epoch from now())::bigint)
        on conflict (tenant_id, app_id, work_id) do update
        set status = 'published',
            published_at = coalesce(work_publications.published_at, now()),
            tags = excluded.tags,
            sort_score = extract(epoch from now())::bigint,
            updated_at = now()
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(tags)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    let work = find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebWorkPublicationResponse { work },
        request_id.to_string(),
    )))
}

pub async fn unpublish_work(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(work_id): Path<Uuid>,
    Json(payload): Json<WebWorkCustomerRequest>,
) -> Result<Json<ApiResponse<WebWorkPublicationResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    ensure_customer_active(&state, server_key.tenant_id, payload.customer_id).await?;
    find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;

    sqlx::query(
        r#"
        update customer_works
        set visibility = 'private',
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and id = $3
          and owner_customer_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(payload.customer_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    sqlx::query(
        r#"
        update work_publications
        set status = 'unpublished',
            updated_at = now()
        where tenant_id = $1
          and app_id = $2
          and work_id = $3
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    let work = find_owner_work(&state, &server_key, work_id, payload.customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebWorkPublicationResponse { work },
        request_id.to_string(),
    )))
}

pub async fn list_gallery(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<WebGalleryQuery>,
) -> Result<Json<ApiResponse<WebWorkListResponse>>, AppError> {
    let server_key = authenticate_web_works_key(&state, &headers).await?;
    if let Some(customer_id) = query.customer_id {
        ensure_customer_active(&state, server_key.tenant_id, customer_id).await?;
    }
    let work_type = query
        .work_type
        .as_deref()
        .map(normalize_work_type)
        .transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let fetch_limit = page_size + 1;
    let items = list_published_gallery(
        &state,
        &server_key,
        query.customer_id,
        work_type.as_deref(),
        page,
        fetch_limit,
    )
    .await?;

    let has_more = items.len() as i64 > page_size;
    let items = items
        .into_iter()
        .take(page_size as usize)
        .collect::<Vec<_>>();
    let meta = ListMeta::new(page, page_size, has_more);
    Ok(Json(ApiResponse::ok(
        WebWorkListResponse {
            works: items.clone(),
            items,
            meta: meta.clone(),
            pagination: meta,
        },
        request_id.to_string(),
    )))
}

pub async fn create_work_for_generated_asset(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
    customer_id: Uuid,
    source_job_id: Option<Uuid>,
    customer_asset_id: Uuid,
    work_type: &str,
    metadata: Value,
) -> Result<Option<Uuid>, AppError> {
    let work_type = normalize_work_type(work_type)?;
    let metadata = normalize_metadata(Some(metadata))?;
    let title = generated_work_title(&work_type);

    let work_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        insert into customer_works (
          tenant_id,
          app_id,
          owner_customer_id,
          source_job_id,
          title,
          work_type,
          visibility,
          primary_asset_id,
          metadata_json
        )
        values ($1, $2, $3, $4, $5, $6, 'private', $7, $8)
        on conflict (tenant_id, app_id, primary_asset_id)
          where deleted_at is null do nothing
        returning id
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(customer_id)
    .bind(source_job_id)
    .bind(title)
    .bind(&work_type)
    .bind(customer_asset_id)
    .bind(metadata)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?;

    let Some(work_id) = work_id else {
        return Ok(None);
    };
    sqlx::query(
        r#"
        insert into work_assets (
          work_id,
          asset_id,
          role,
          sort_order
        )
        values ($1, $2, 'primary', 0)
        on conflict do nothing
        "#,
    )
    .bind(work_id)
    .bind(customer_asset_id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(Some(work_id))
}

async fn list_owner_works(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    work_type: Option<&str>,
    visibility: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<WebWork>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, WebWork>(&format!(
        "{} where w.tenant_id = $1
          and w.app_id = $2
          and w.owner_customer_id = $3
          and w.deleted_at is null
          and w.status = 'active'
          and ($4::text is null or w.work_type = $4)
          and ($5::text is null or w.visibility = $5)
        order by w.created_at desc, w.id desc
        limit $6 offset $7",
        work_select_sql()
    ))
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(work_type)
    .bind(visibility)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn list_favorite_works(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    work_type: Option<&str>,
    visibility: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<WebWork>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, WebWork>(&format!(
        "{} join work_favorites filter_f
          on filter_f.work_id = w.id
          and filter_f.tenant_id = w.tenant_id
          and filter_f.app_id = w.app_id
          and filter_f.customer_id = $3
          and filter_f.deleted_at is null
        where w.tenant_id = $1
          and w.app_id = $2
          and w.deleted_at is null
          and w.status = 'active'
          and ($4::text is null or w.work_type = $4)
          and ($5::text is null or w.visibility = $5)
        order by filter_f.created_at desc, w.id desc
        limit $6 offset $7",
        work_select_sql()
    ))
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(work_type)
    .bind(visibility)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn list_published_gallery(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    current_customer_id: Option<Uuid>,
    work_type: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<WebWork>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, WebWork>(&format!(
        "{} join work_publications gallery_p
          on gallery_p.work_id = w.id
          and gallery_p.tenant_id = w.tenant_id
          and gallery_p.app_id = w.app_id
          and gallery_p.status = 'published'
        where w.tenant_id = $1
          and w.app_id = $2
          and w.deleted_at is null
          and w.status = 'active'
          and w.visibility = 'public'
          and ($4::text is null or w.work_type = $4)
        order by gallery_p.sort_score desc, gallery_p.published_at desc nulls last, w.created_at desc
        limit $5 offset $6",
        work_select_sql()
    ))
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(current_customer_id)
    .bind(work_type)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_owner_work(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    work_id: Uuid,
    customer_id: Uuid,
) -> Result<WebWork, AppError> {
    sqlx::query_as::<_, WebWork>(&format!(
        "{} where w.tenant_id = $1
          and w.app_id = $2
          and w.owner_customer_id = $3
          and w.id = $4
          and w.deleted_at is null
          and w.status = 'active'",
        work_select_sql()
    ))
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(work_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("work not found"))
}

async fn find_visible_work(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    work_id: Uuid,
    customer_id: Uuid,
) -> Result<WebWork, AppError> {
    sqlx::query_as::<_, WebWork>(&format!(
        "{} where w.tenant_id = $1
          and w.app_id = $2
          and w.id = $4
          and w.deleted_at is null
          and w.status = 'active'
          and (
            w.owner_customer_id = $3
            or exists (
              select 1
              from work_publications visible_p
              where visible_p.tenant_id = w.tenant_id
                and visible_p.app_id = w.app_id
                and visible_p.work_id = w.id
                and visible_p.status = 'published'
            )
          )",
        work_select_sql()
    ))
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(customer_id)
    .bind(work_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("work not found"))
}

fn work_select_sql() -> &'static str {
    r#"
    select
      w.id,
      w.owner_customer_id,
      w.source_job_id,
      w.title,
      w.description,
      w.work_type,
      w.status,
      w.visibility,
      w.cover_asset_id,
      w.primary_asset_id,
      primary_asset.public_url as primary_asset_url,
      primary_asset.mime_type as primary_asset_mime_type,
      primary_asset.file_size as primary_asset_file_size,
      cover_asset.public_url as cover_asset_url,
      w.metadata_json,
      coalesce(favorite_stats.favorite_count, 0)::bigint as favorite_count,
      exists (
        select 1
        from work_favorites current_favorite
        where current_favorite.tenant_id = w.tenant_id
          and current_favorite.app_id = w.app_id
          and current_favorite.work_id = w.id
          and current_favorite.customer_id = $3
          and current_favorite.deleted_at is null
      ) as favorited,
      nullif(w.metadata_json->>'sourceMode', '') as source_mode,
      coalesce(
        case when w.metadata_json->>'referenceCount' ~ '^[0-9]+$'
          then (w.metadata_json->>'referenceCount')::bigint
        end,
        0
      )::bigint as reference_count,
      coalesce(
        case when lower(w.metadata_json->>'hasFirstFrame') in ('true', 'false')
          then (w.metadata_json->>'hasFirstFrame')::boolean
        end,
        false
      ) as has_first_frame,
      coalesce(
        case when lower(w.metadata_json->>'hasLastFrame') in ('true', 'false')
          then (w.metadata_json->>'hasLastFrame')::boolean
        end,
        false
      ) as has_last_frame,
      publication.status as publication_status,
      publication.published_at,
      current_favorite.created_at as favorited_at,
      current_download.downloaded_at as downloaded_at,
      coalesce(publication.tags, '[]'::jsonb) as publication_tags,
      w.created_at,
      w.updated_at
    from customer_works w
    join customer_assets primary_asset
      on primary_asset.id = w.primary_asset_id
      and primary_asset.tenant_id = w.tenant_id
      and primary_asset.app_id = w.app_id
    left join customer_assets cover_asset
      on cover_asset.id = w.cover_asset_id
      and cover_asset.tenant_id = w.tenant_id
      and cover_asset.app_id = w.app_id
    left join work_publications publication
      on publication.work_id = w.id
      and publication.tenant_id = w.tenant_id
      and publication.app_id = w.app_id
    left join work_favorites current_favorite
      on current_favorite.work_id = w.id
      and current_favorite.tenant_id = w.tenant_id
      and current_favorite.app_id = w.app_id
      and current_favorite.customer_id = $3
      and current_favorite.deleted_at is null
    left join work_downloads current_download
      on current_download.work_id = w.id
      and current_download.tenant_id = w.tenant_id
      and current_download.app_id = w.app_id
      and current_download.customer_id = $3
    left join lateral (
      select count(*)::bigint as favorite_count
      from work_favorites f
      where f.tenant_id = w.tenant_id
        and f.app_id = w.app_id
        and f.work_id = w.id
        and f.deleted_at is null
    ) favorite_stats on true
    "#
}

async fn active_favorite_at(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    work_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<DateTime<Utc>>, AppError> {
    sqlx::query_scalar::<_, DateTime<Utc>>(
        r#"
        select created_at
        from work_favorites
        where tenant_id = $1
          and app_id = $2
          and work_id = $3
          and customer_id = $4
          and deleted_at is null
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(server_key.app_id)
    .bind(work_id)
    .bind(customer_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)
}

async fn ensure_asset_belongs_to_customer(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    asset_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(asset_id) = asset_id else {
        return Ok(());
    };
    let asset = sqlx::query_as::<_, WorkAssetOwner>(
        r#"
        select
          customer_id
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
    if asset.customer_id != customer_id {
        return Err(AppError::forbidden("asset does not belong to customer"));
    }

    Ok(())
}

async fn authenticate_web_works_key(
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

fn normalize_title(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_TITLE_LEN
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(AppError::validation_failed("work title is invalid"));
    }

    Ok(value.to_owned())
}

fn normalize_optional_description(value: Option<String>) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_DESCRIPTION_LEN || value.contains('\0') {
        return Err(AppError::validation_failed("work description is invalid"));
    }

    Ok(Some(value.to_owned()))
}

fn normalize_work_type(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "image" | "video" | "audio" | "file" => Ok(value),
        _ => Err(AppError::validation_failed("work type is invalid")),
    }
}

fn normalize_visibility(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "private" | "public" => Ok(value),
        _ => Err(AppError::validation_failed("work visibility is invalid")),
    }
}

fn normalize_metadata(value: Option<Value>) -> Result<Value, AppError> {
    let value = value.unwrap_or_else(|| json!({}));
    if !value.is_object() {
        return Err(AppError::validation_failed(
            "work metadata must be an object",
        ));
    }

    Ok(value)
}

fn normalize_tags(tags: Option<Vec<String>>) -> Result<Value, AppError> {
    let Some(tags) = tags else {
        return Ok(json!([]));
    };
    if tags.len() > MAX_TAGS {
        return Err(AppError::validation_failed(
            "work publication tags are invalid",
        ));
    }
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.len() > MAX_TAG_LEN || tag.contains('\0') || tag.chars().any(char::is_control) {
            return Err(AppError::validation_failed(
                "work publication tags are invalid",
            ));
        }
        if !normalized.iter().any(|existing| existing == tag) {
            normalized.push(tag.to_owned());
        }
    }

    Ok(json!(normalized))
}

fn generated_work_title(work_type: &str) -> &'static str {
    match work_type {
        "video" => "AI 生成视频",
        "image" => "AI 生成图片",
        "audio" => "AI 生成音频",
        _ => "AI 生成作品",
    }
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("web work database error: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        normalize_metadata, normalize_optional_description, normalize_tags, normalize_title,
        normalize_visibility, normalize_work_type,
    };

    #[test]
    fn work_fields_are_validated() {
        assert_eq!(normalize_title("  测试作品  ").unwrap(), "测试作品");
        assert!(normalize_title("").is_err());
        assert_eq!(normalize_work_type("Video").unwrap(), "video");
        assert!(normalize_work_type("gallery").is_err());
        assert_eq!(normalize_visibility("PUBLIC").unwrap(), "public");
        assert!(normalize_visibility("team").is_err());
    }

    #[test]
    fn description_metadata_and_tags_are_validated() {
        assert_eq!(normalize_optional_description(None).unwrap(), None);
        assert_eq!(
            normalize_optional_description(Some("  描述  ".to_owned())).unwrap(),
            Some("描述".to_owned())
        );
        assert!(normalize_metadata(Some(json!({ "a": 1 }))).is_ok());
        assert!(normalize_metadata(Some(json!([]))).is_err());
        assert_eq!(
            normalize_tags(Some(vec!["赛博".to_owned(), "赛博".to_owned()])).unwrap(),
            json!(["赛博"])
        );
    }
}
