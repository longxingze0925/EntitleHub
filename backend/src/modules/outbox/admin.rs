use axum::{
    extract::{Path, Query, State},
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
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

#[derive(Debug, Clone, Deserialize)]
pub struct OutboxEventListQuery {
    pub status: Option<String>,
    pub event_type: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutboxEventListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutboxEventSummary {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub event_type: String,
    pub payload: Value,
    pub status: String,
    pub attempts: i32,
    pub next_run_at: DateTime<Utc>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct OutboxEventListResponse {
    pub items: Vec<OutboxEventSummary>,
    pub meta: OutboxEventListMeta,
}

#[derive(Debug, Serialize)]
pub struct OutboxEventMutationResponse {
    pub event: OutboxEventSummary,
}

#[derive(Debug, Clone, FromRow)]
struct OutboxEventRecord {
    id: Uuid,
    tenant_id: Option<Uuid>,
    event_type: String,
    payload: Value,
    status: String,
    attempts: i32,
    next_run_at: DateTime<Utc>,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    processed_at: Option<DateTime<Utc>>,
}

pub async fn list_outbox_events(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<OutboxEventListQuery>,
) -> Result<Json<ApiResponse<OutboxEventListResponse>>, AppError> {
    ensure_admin_permission(&admin, "security:view_events")?;
    validate_outbox_event_query(&query)?;

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let events = list_events(&state, admin.tenant_id, &query, page, page_size).await?;
    let items = events.into_iter().map(OutboxEventSummary::from).collect();

    Ok(Json(ApiResponse::ok(
        OutboxEventListResponse {
            items,
            meta: OutboxEventListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

pub async fn retry_outbox_event(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(event_id): Path<Uuid>,
) -> Result<Json<ApiResponse<OutboxEventMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "security:retry_event")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_event_for_update(&mut transaction, admin.tenant_id, event_id).await?;
    ensure_retryable_status(&before.status)?;
    let event = retry_event_in_transaction(&mut transaction, admin.tenant_id, event_id).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "outbox_event.retry",
            resource_type: "outbox_event",
            resource_id: Some(event.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(outbox_event_audit_json(&before)),
            after_json: Some(outbox_event_audit_json(&event)),
            metadata_json: json!({
                "event_type": &event.event_type,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        OutboxEventMutationResponse {
            event: OutboxEventSummary::from(event),
        },
        request_id.to_string(),
    )))
}

async fn list_events(
    state: &AppState,
    tenant_id: Uuid,
    query: &OutboxEventListQuery,
    page: u32,
    page_size: u32,
) -> Result<Vec<OutboxEventRecord>, AppError> {
    let offset = ((page - 1) * page_size) as i64;
    let limit = page_size as i64;
    let status = clean_optional(query.status.as_deref());
    let event_type = clean_optional(query.event_type.as_deref());

    sqlx::query_as::<_, OutboxEventRecord>(
        r#"
        select
          id,
          tenant_id,
          event_type,
          payload,
          status,
          attempts,
          next_run_at,
          last_error,
          created_at,
          processed_at
        from outbox_events
        where tenant_id = $1
          and ($2::text is null or status = $2)
          and ($3::text is null or event_type = $3)
        order by created_at desc, id desc
        limit $4 offset $5
        "#,
    )
    .bind(tenant_id)
    .bind(status)
    .bind(event_type)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_event_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    event_id: Uuid,
) -> Result<OutboxEventRecord, AppError> {
    sqlx::query_as::<_, OutboxEventRecord>(&outbox_event_select_sql(
        "where tenant_id = $1 and id = $2 for update",
    ))
    .bind(tenant_id)
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("outbox event not found"))
}

async fn retry_event_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    event_id: Uuid,
) -> Result<OutboxEventRecord, AppError> {
    sqlx::query_as::<_, OutboxEventRecord>(
        r#"
        update outbox_events
        set
          status = 'pending',
          attempts = 0,
          next_run_at = now(),
          last_error = null,
          processed_at = null
        where tenant_id = $1
          and id = $2
          and status = 'failed'
        returning
          id,
          tenant_id,
          event_type,
          payload,
          status,
          attempts,
          next_run_at,
          last_error,
          created_at,
          processed_at
        "#,
    )
    .bind(tenant_id)
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::conflict("only failed outbox events can be retried"))
}

fn outbox_event_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
          id,
          tenant_id,
          event_type,
          payload,
          status,
          attempts,
          next_run_at,
          last_error,
          created_at,
          processed_at
        from outbox_events
        {where_clause}
        "#
    )
}

pub fn validate_outbox_event_query(query: &OutboxEventListQuery) -> Result<(), AppError> {
    if let Some(status) = clean_optional(query.status.as_deref()) {
        validate_code_filter(status, "status")?;
    }
    if let Some(event_type) = clean_optional(query.event_type.as_deref()) {
        validate_code_filter(event_type, "event_type")?;
    }

    Ok(())
}

impl From<OutboxEventRecord> for OutboxEventSummary {
    fn from(event: OutboxEventRecord) -> Self {
        Self {
            id: event.id,
            tenant_id: event.tenant_id,
            event_type: event.event_type,
            payload: sanitize_payload(&event.payload),
            status: event.status,
            attempts: event.attempts,
            next_run_at: event.next_run_at,
            last_error: event.last_error,
            created_at: event.created_at,
            processed_at: event.processed_at,
        }
    }
}

fn sanitize_payload(payload: &Value) -> Value {
    let mut payload = payload.clone();
    let Value::Object(map) = &mut payload else {
        return payload;
    };

    if map.remove("body_envelope").is_some() {
        map.insert("body_envelope_redacted".to_owned(), json!(true));
    }

    payload
}

fn outbox_event_audit_json(event: &OutboxEventRecord) -> Value {
    json!({
        "id": event.id,
        "tenant_id": &event.tenant_id,
        "event_type": &event.event_type,
        "status": &event.status,
        "attempts": event.attempts,
        "next_run_at": &event.next_run_at,
        "last_error": &event.last_error,
        "processed_at": &event.processed_at,
    })
}

fn ensure_retryable_status(status: &str) -> Result<(), AppError> {
    if status == "failed" {
        return Ok(());
    }

    Err(AppError::conflict(
        "only failed outbox events can be retried",
    ))
}

fn clean_optional(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn validate_code_filter(value: &str, field: &'static str) -> Result<(), AppError> {
    let valid = value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'));
    if valid {
        return Ok(());
    }

    Err(AppError::validation_failed(format!("{field} is invalid")))
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
    AppError::dependency(format!("outbox event database error: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ensure_retryable_status, sanitize_payload, validate_outbox_event_query,
        OutboxEventListQuery,
    };

    #[test]
    fn outbox_query_rejects_invalid_filters() {
        let query = OutboxEventListQuery {
            status: Some("bad status".to_owned()),
            event_type: None,
            page: None,
            page_size: None,
        };
        assert!(validate_outbox_event_query(&query).is_err());

        let query = OutboxEventListQuery {
            status: Some("pending".to_owned()),
            event_type: Some("email.admin_password_reset".to_owned()),
            page: None,
            page_size: None,
        };
        assert!(validate_outbox_event_query(&query).is_ok());
    }

    #[test]
    fn sanitize_payload_redacts_email_body_envelope() {
        let payload = json!({
            "kind": "email",
            "to": "user@example.com",
            "subject": "Verify",
            "body_envelope": {
                "ciphertext": "secret"
            }
        });
        let sanitized = sanitize_payload(&payload);

        assert_eq!(sanitized["to"], "user@example.com");
        assert!(sanitized.get("body_envelope").is_none());
        assert_eq!(sanitized["body_envelope_redacted"], true);
    }

    #[test]
    fn retry_only_allows_failed_status() {
        assert!(ensure_retryable_status("failed").is_ok());
        assert!(ensure_retryable_status("pending").is_err());
        assert!(ensure_retryable_status("processed").is_err());
    }
}
