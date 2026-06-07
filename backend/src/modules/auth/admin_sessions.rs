use axum::{
    extract::{Path, State},
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
        audit::{self, AuditLogInput},
        auth::{
            admin_session::{
                find_admin_session_for_update_in_transaction,
                revoke_admin_refresh_tokens_for_session_in_transaction,
                revoke_admin_session_in_transaction, AdminSessionRepository, AdminSessionSummary,
            },
            session::AdminContext,
        },
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct AdminSessionListResponse {
    pub items: Vec<AdminSessionSummaryResponse>,
}

#[derive(Debug, Serialize)]
pub struct AdminSessionSummaryResponse {
    pub id: Uuid,
    pub current: bool,
    pub status: String,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AdminSessionRevokeResponse {
    pub revoked: bool,
    pub session_id: Uuid,
    pub revoked_refresh_tokens: u64,
}

pub async fn list_sessions(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<AdminSessionListResponse>>, AppError> {
    let now = Utc::now();
    let items = AdminSessionRepository::new(state.db.clone())
        .list_for_member(admin.tenant_id, admin.team_member_id)
        .await?
        .into_iter()
        .map(|session| session_response(session, admin.session_id, now))
        .collect();

    Ok(Json(ApiResponse::ok(
        AdminSessionListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn revoke_session(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(session_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AdminSessionRevokeResponse>>, AppError> {
    if session_id == admin.session_id {
        return Err(AppError::business_rule_failed(
            "current session cannot be revoked from session management",
        ));
    }

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let target = find_admin_session_for_update_in_transaction(&mut transaction, session_id)
        .await?
        .ok_or_else(|| AppError::not_found("admin session not found"))?;

    if target.tenant_id != admin.tenant_id || target.team_member_id != admin.team_member_id {
        return Err(AppError::not_found("admin session not found"));
    }
    if target.revoked_at.is_some() {
        return Err(AppError::already_revoked("admin session already revoked"));
    }

    let revoked = revoke_admin_session_in_transaction(&mut transaction, session_id).await?;
    let revoked_refresh_tokens =
        revoke_admin_refresh_tokens_for_session_in_transaction(&mut transaction, session_id)
            .await?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "admin_session.revoke",
            resource_type: "admin_session",
            resource_id: Some(session_id),
            ip: Some(rate_limit::client_ip(&headers)),
            user_agent: headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "id": target.id,
                "created_at": target.created_at,
                "last_seen_at": target.last_seen_at,
                "expires_at": target.expires_at,
                "revoked_at": target.revoked_at,
            })),
            after_json: Some(json!({
                "id": session_id,
                "revoked": revoked,
            })),
            metadata_json: json!({
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AdminSessionRevokeResponse {
            revoked,
            session_id,
            revoked_refresh_tokens,
        },
        request_id.to_string(),
    )))
}

fn session_response(
    session: AdminSessionSummary,
    current_session_id: Uuid,
    now: DateTime<Utc>,
) -> AdminSessionSummaryResponse {
    let status = if session.revoked_at.is_some() {
        "revoked"
    } else if session.expires_at <= now {
        "expired"
    } else {
        "active"
    };

    AdminSessionSummaryResponse {
        current: session.id == current_session_id,
        status: status.to_owned(),
        id: session.id,
        ip: session.ip,
        user_agent: session.user_agent,
        created_at: session.created_at,
        last_seen_at: session.last_seen_at,
        expires_at: session.expires_at,
        revoked_at: session.revoked_at,
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("admin session management database error: {error}"))
}
