use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiAsset {
    pub id: Uuid,
    pub usage_id: Option<Uuid>,
    pub customer_id: Option<Uuid>,
    pub customer_email: Option<String>,
    pub customer_name: Option<String>,
    pub provider_name: Option<String>,
    pub model_code: Option<String>,
    pub asset_type: String,
    pub status: String,
    pub public_url: Option<String>,
    pub mime_type: Option<String>,
    pub file_size: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub deleted_by: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct AiAssetListResponse {
    pub items: Vec<AiAsset>,
    pub meta: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct AiAssetResponse {
    pub asset: AiAsset,
}

#[derive(Debug, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct AiAssetListQuery {
    pub status: Option<String>,
    pub asset_type: Option<String>,
    pub customer_id: Option<Uuid>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

pub async fn list_ai_assets(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiAssetListQuery>,
) -> Result<Json<ApiResponse<AiAssetListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;
    let status = query.status.as_deref().map(normalize_status).transpose()?;
    let asset_type = query
        .asset_type
        .as_deref()
        .map(normalize_asset_type)
        .transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_assets(
        &state,
        admin.tenant_id,
        status.as_deref(),
        asset_type.as_deref(),
        query.customer_id,
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiAssetListResponse {
            items,
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

pub async fn delete_ai_asset(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(asset_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AiAssetResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:asset:delete")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_asset_for_update(&mut transaction, admin.tenant_id, asset_id).await?;
    if before.status == "deleted" || before.deleted_at.is_some() {
        return Err(AppError::business_rule_failed("ai asset already deleted"));
    }
    mark_asset_deleted(
        &mut transaction,
        admin.tenant_id,
        asset_id,
        admin.team_member_id,
    )
    .await?;
    let after = find_asset_for_update(&mut transaction, admin.tenant_id, asset_id).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_asset.delete",
            resource_type: "ai_asset",
            resource_id: Some(asset_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(asset_audit_json(&before)),
            after_json: Some(asset_audit_json(&after)),
            metadata_json: json!({
                "usage_id": before.usage_id,
                "asset_type": &before.asset_type,
                "status": &before.status,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AiAssetResponse { asset: after },
        request_id.to_string(),
    )))
}

async fn list_assets(
    state: &AppState,
    tenant_id: Uuid,
    status: Option<&str>,
    asset_type: Option<&str>,
    customer_id: Option<Uuid>,
    page: i64,
    page_size: i64,
) -> Result<Vec<AiAsset>, AppError> {
    let offset = (page - 1) * page_size;

    sqlx::query_as::<_, AiAsset>(&asset_select_sql(
        r#"
        where a.tenant_id = $1
          and ($2::text is null or a.status = $2)
          and ($3::text is null or a.asset_type = $3)
          and ($4::uuid is null or u.customer_id = $4)
          and ($2::text is not null or a.status <> 'deleted')
        order by a.created_at desc, a.id desc
        limit $5 offset $6
        "#,
    ))
    .bind(tenant_id)
    .bind(status)
    .bind(asset_type)
    .bind(customer_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_asset_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    asset_id: Uuid,
) -> Result<AiAsset, AppError> {
    sqlx::query_as::<_, AiAsset>(&format!(
        "{} {}",
        asset_select_sql(
            r#"
        where a.tenant_id = $1
          and a.id = $2
        "#,
        ),
        "for update of a"
    ))
    .bind(tenant_id)
    .bind(asset_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai asset not found"))
}

async fn mark_asset_deleted(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    asset_id: Uuid,
    deleted_by: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_assets
        set status = 'deleted',
            deleted_at = now(),
            deleted_by = $3,
            updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
          and status <> 'deleted'
        "#,
    )
    .bind(tenant_id)
    .bind(asset_id)
    .bind(deleted_by)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

fn asset_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
          a.id,
          a.usage_id,
          u.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          p.name as provider_name,
          m.code as model_code,
          a.asset_type,
          a.status,
          a.public_url,
          a.mime_type,
          a.file_size,
          a.created_at,
          a.updated_at,
          a.deleted_at,
          a.deleted_by
        from ai_assets a
        left join ai_usage_records u
          on u.id = a.usage_id
         and u.tenant_id = a.tenant_id
        left join customers c
          on c.id = u.customer_id
         and c.tenant_id = a.tenant_id
        left join ai_providers p
          on p.id = u.provider_id
         and p.tenant_id = a.tenant_id
        left join ai_models m
          on m.id = u.model_id
         and m.tenant_id = a.tenant_id
        {where_clause}
        "#
    )
}

fn normalize_status(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "caching" | "ready" | "failed" | "deleted" => Ok(value),
        _ => Err(AppError::validation_failed("ai asset status is invalid")),
    }
}

fn normalize_asset_type(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "image" | "video" | "audio" | "file" => Ok(value),
        _ => Err(AppError::validation_failed("ai asset type is invalid")),
    }
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
}

fn asset_audit_json(record: &AiAsset) -> Value {
    json!({
        "id": record.id,
        "usage_id": record.usage_id,
        "customer_id": record.customer_id,
        "provider_name": &record.provider_name,
        "model_code": &record.model_code,
        "asset_type": &record.asset_type,
        "status": &record.status,
        "public_url": &record.public_url,
        "mime_type": &record.mime_type,
        "file_size": record.file_size,
        "created_at": &record.created_at,
        "updated_at": &record.updated_at,
        "deleted_at": &record.deleted_at,
        "deleted_by": record.deleted_by,
    })
}

fn ensure_admin_permission(admin: &AdminContext, permission_code: &str) -> Result<(), AppError> {
    if admin
        .permissions
        .iter()
        .any(|permission| permission == permission_code)
    {
        return Ok(());
    }

    Err(AppError::forbidden(format!(
        "missing permission: {permission_code}"
    )))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("ai asset database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{normalize_asset_type, normalize_page, normalize_status};

    #[test]
    fn asset_filters_are_validated() {
        assert_eq!(normalize_status("Ready").expect("status"), "ready");
        assert_eq!(normalize_asset_type("Image").expect("type"), "image");
        assert!(normalize_status("unknown").is_err());
        assert!(normalize_asset_type("folder").is_err());
    }

    #[test]
    fn page_size_is_bounded() {
        assert_eq!(normalize_page(Some(0), Some(500)), (1, 100));
    }
}
