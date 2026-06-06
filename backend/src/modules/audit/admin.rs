use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{
            self, validate_audit_log_query, AuditLogDetail, AuditLogInput, AuditLogListMeta,
            AuditLogListQuery, AuditLogRepository, AuditLogSummary,
        },
        auth::session::AdminContext,
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct AuditLogListResponse {
    pub items: Vec<AuditLogSummary>,
    pub meta: AuditLogListMeta,
}

#[derive(Debug, Serialize)]
pub struct AuditLogDetailResponse {
    pub audit_log: AuditLogDetail,
}

#[derive(Debug, Serialize)]
pub struct AuditLogExportResponse {
    pub items: Vec<AuditLogDetail>,
    pub exported_at: DateTime<Utc>,
    pub limit: u32,
}

pub async fn list_audit_logs(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AuditLogListQuery>,
) -> Result<Json<ApiResponse<AuditLogListResponse>>, AppError> {
    ensure_admin_permission(&admin, "audit:read")?;
    validate_audit_log_query(&query)?;

    let logs = AuditLogRepository::new(state.db.clone())
        .list(admin.tenant_id, &query)
        .await?;
    let items = logs.into_iter().map(AuditLogSummary::from).collect();

    Ok(Json(ApiResponse::ok(
        AuditLogListResponse {
            items,
            meta: AuditLogListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn get_audit_log(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(audit_log_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AuditLogDetailResponse>>, AppError> {
    ensure_admin_permission(&admin, "audit:read")?;

    let audit_log = AuditLogRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, audit_log_id)
        .await?
        .ok_or_else(|| AppError::not_found("audit log not found"))?;

    Ok(Json(ApiResponse::ok(
        AuditLogDetailResponse {
            audit_log: audit_log.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn export_audit_logs(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<AuditLogListQuery>,
) -> Result<Json<ApiResponse<AuditLogExportResponse>>, AppError> {
    ensure_admin_permission(&admin, "audit:export")?;
    validate_audit_log_query(&query)?;

    let logs = AuditLogRepository::new(state.db.clone())
        .export(admin.tenant_id, &query)
        .await?;
    let exported_count = logs.len();
    let items = logs.into_iter().map(AuditLogDetail::from).collect();

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "audit.export",
            resource_type: "audit_log",
            resource_id: None,
            ip: Some(rate_limit::client_ip(&headers)),
            user_agent: headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: None,
            metadata_json: json!({
                "filters": query,
                "exported_count": exported_count,
                "limit": 1_000,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AuditLogExportResponse {
            items,
            exported_at: Utc::now(),
            limit: 1_000,
        },
        request_id.to_string(),
    )))
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
    AppError::dependency(format!("audit admin database error: {error}"))
}
