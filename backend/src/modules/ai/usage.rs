use axum::{
    extract::{Query, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::auth::session::AdminContext,
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiUsageRecord {
    pub id: Uuid,
    pub customer_id: Option<Uuid>,
    pub customer_email: Option<String>,
    pub customer_name: Option<String>,
    pub provider_name: Option<String>,
    pub model_code: Option<String>,
    pub request_id: Option<String>,
    pub endpoint: String,
    pub status: String,
    pub provider_status: Option<String>,
    pub provider_request_id: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub charged_minor: i64,
    pub refunded_minor: i64,
    pub provider_cost_minor: Option<i64>,
    pub currency: String,
    pub price_snapshot: Value,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AiUsageRecordListResponse {
    pub items: Vec<AiUsageRecord>,
    pub meta: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct AiUsageRecordListQuery {
    pub status: Option<String>,
    pub customer_id: Option<Uuid>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

pub async fn list_ai_usage_records(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiUsageRecordListQuery>,
) -> Result<Json<ApiResponse<AiUsageRecordListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;
    let status = query.status.as_deref().map(normalize_status).transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_usage_records(
        &state,
        admin.tenant_id,
        status.as_deref(),
        query.customer_id,
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiUsageRecordListResponse {
            items,
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

async fn list_usage_records(
    state: &AppState,
    tenant_id: Uuid,
    status: Option<&str>,
    customer_id: Option<Uuid>,
    page: i64,
    page_size: i64,
) -> Result<Vec<AiUsageRecord>, AppError> {
    let offset = (page - 1) * page_size;

    sqlx::query_as::<_, AiUsageRecord>(
        r#"
        select
          u.id,
          u.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          p.name as provider_name,
          m.code as model_code,
          u.request_id,
          u.endpoint,
          u.status,
          u.provider_status,
          u.provider_request_id,
          u.prompt_tokens,
          u.completion_tokens,
          u.total_tokens,
          u.charged_minor,
          u.refunded_minor,
          u.provider_cost_minor,
          coalesce(u.price_snapshot_json->>'currency', m.currency, 'CNY') as currency,
          u.price_snapshot_json as price_snapshot,
          u.metadata_json as metadata,
          u.created_at,
          u.completed_at
        from ai_usage_records u
        left join customers c
          on c.id = u.customer_id
          and c.tenant_id = u.tenant_id
        left join ai_providers p
          on p.id = u.provider_id
          and p.tenant_id = u.tenant_id
        left join ai_models m
          on m.id = u.model_id
          and m.tenant_id = u.tenant_id
        where u.tenant_id = $1
          and ($2::text is null or u.status = $2)
          and ($3::uuid is null or u.customer_id = $3)
        order by u.created_at desc, u.id desc
        limit $4 offset $5
        "#,
    )
    .bind(tenant_id)
    .bind(status)
    .bind(customer_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

fn normalize_status(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "pending" | "running" | "succeeded" | "failed" | "refunded" => Ok(value),
        _ => Err(AppError::validation_failed("ai usage status is invalid")),
    }
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
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
    AppError::dependency(format!("ai usage database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{normalize_page, normalize_status};

    #[test]
    fn usage_status_is_validated() {
        assert_eq!(normalize_status("Succeeded").expect("status"), "succeeded");
        assert!(normalize_status("unknown").is_err());
    }

    #[test]
    fn page_size_is_bounded() {
        assert_eq!(normalize_page(Some(0), Some(500)), (1, 100));
    }
}
