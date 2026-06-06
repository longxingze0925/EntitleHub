use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::{FromRow, PgPool};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::AppError;

pub mod admin;

pub struct AuditLogInput {
    pub tenant_id: Option<Uuid>,
    pub actor_type: &'static str,
    pub actor_id: Option<Uuid>,
    pub action: &'static str,
    pub resource_type: &'static str,
    pub resource_id: Option<Uuid>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
    pub before_json: Option<Value>,
    pub after_json: Option<Value>,
    pub metadata_json: Value,
}

#[derive(Debug, Clone, FromRow)]
pub struct AuditLog {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub actor_type: String,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
    pub before_json: Option<Value>,
    pub after_json: Option<Value>,
    pub metadata_json: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuditLogListQuery {
    pub actor_id: Option<Uuid>,
    pub action: Option<String>,
    pub resource_type: Option<String>,
    pub resource_id: Option<Uuid>,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditLogListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct AuditLogSummary {
    pub id: Uuid,
    pub actor_type: String,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AuditLogDetail {
    pub id: Uuid,
    pub actor_type: String,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<Uuid>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
    pub before_json: Option<Value>,
    pub after_json: Option<Value>,
    pub metadata_json: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AuditLogRepository {
    pool: PgPool,
}

impl AuditLogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        query: &AuditLogListQuery,
    ) -> Result<Vec<AuditLog>, AppError> {
        validate_audit_log_query(query)?;
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;
        self.list_with_bounds(tenant_id, query, limit, offset).await
    }

    pub async fn export(
        &self,
        tenant_id: Uuid,
        query: &AuditLogListQuery,
    ) -> Result<Vec<AuditLog>, AppError> {
        validate_audit_log_query(query)?;
        self.list_with_bounds(tenant_id, query, 1_000, 0).await
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Result<Option<AuditLog>, AppError> {
        sqlx::query_as::<_, AuditLog>(
            r#"
            select
              id,
              tenant_id,
              actor_type,
              actor_id,
              action,
              resource_type,
              resource_id,
              ip::text as ip,
              user_agent,
              request_id,
              before_json,
              after_json,
              metadata_json,
              created_at
            from audit_logs
            where tenant_id = $1
              and id = $2
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    async fn list_with_bounds(
        &self,
        tenant_id: Uuid,
        query: &AuditLogListQuery,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<AuditLog>, AppError> {
        let action = clean_optional(query.action.as_deref());
        let resource_type = clean_optional(query.resource_type.as_deref());

        sqlx::query_as::<_, AuditLog>(
            r#"
            select
              id,
              tenant_id,
              actor_type,
              actor_id,
              action,
              resource_type,
              resource_id,
              ip::text as ip,
              user_agent,
              request_id,
              before_json,
              after_json,
              metadata_json,
              created_at
            from audit_logs
            where tenant_id = $1
              and ($2::uuid is null or actor_id = $2)
              and ($3::text is null or action = $3)
              and ($4::text is null or resource_type = $4)
              and ($5::uuid is null or resource_id = $5)
              and ($6::timestamptz is null or created_at >= $6)
              and ($7::timestamptz is null or created_at <= $7)
            order by created_at desc, id desc
            limit $8 offset $9
            "#,
        )
        .bind(tenant_id)
        .bind(query.actor_id)
        .bind(action)
        .bind(resource_type)
        .bind(query.resource_id)
        .bind(query.start_at)
        .bind(query.end_at)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

pub async fn record(
    transaction: &mut Transaction<'_, Postgres>,
    input: AuditLogInput,
) -> Result<(), AppError> {
    let before_json = input.before_json.map(sanitize_audit_value);
    let after_json = input.after_json.map(sanitize_audit_value);
    let metadata_json = sanitize_audit_value(input.metadata_json);

    sqlx::query(
        r#"
        insert into audit_logs (
          id,
          tenant_id,
          actor_type,
          actor_id,
          action,
          resource_type,
          resource_id,
          ip,
          user_agent,
          request_id,
          before_json,
          after_json,
          metadata_json
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8::inet, $9, $10, $11, $12, $13)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(input.tenant_id)
    .bind(input.actor_type)
    .bind(input.actor_id)
    .bind(input.action)
    .bind(input.resource_type)
    .bind(input.resource_id)
    .bind(input.ip)
    .bind(input.user_agent)
    .bind(input.request_id)
    .bind(before_json)
    .bind(after_json)
    .bind(metadata_json)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

pub fn validate_audit_log_query(query: &AuditLogListQuery) -> Result<(), AppError> {
    if let (Some(start_at), Some(end_at)) = (query.start_at, query.end_at) {
        if end_at < start_at {
            return Err(AppError::validation_failed(
                "end_at must be greater than or equal to start_at",
            ));
        }
    }

    if let Some(action) = clean_optional(query.action.as_deref()) {
        validate_code_filter(action, "action")?;
    }
    if let Some(resource_type) = clean_optional(query.resource_type.as_deref()) {
        validate_code_filter(resource_type, "resource_type")?;
    }

    Ok(())
}

impl From<AuditLog> for AuditLogSummary {
    fn from(log: AuditLog) -> Self {
        Self {
            id: log.id,
            actor_type: log.actor_type,
            actor_id: log.actor_id,
            action: log.action,
            resource_type: log.resource_type,
            resource_id: log.resource_id,
            ip: log.ip,
            user_agent: log.user_agent,
            request_id: log.request_id,
            created_at: log.created_at,
        }
    }
}

impl From<AuditLog> for AuditLogDetail {
    fn from(log: AuditLog) -> Self {
        Self {
            id: log.id,
            actor_type: log.actor_type,
            actor_id: log.actor_id,
            action: log.action,
            resource_type: log.resource_type,
            resource_id: log.resource_id,
            ip: log.ip,
            user_agent: log.user_agent,
            request_id: log.request_id,
            before_json: log.before_json.map(sanitize_audit_value),
            after_json: log.after_json.map(sanitize_audit_value),
            metadata_json: sanitize_audit_value(log.metadata_json),
            created_at: log.created_at,
        }
    }
}

fn sanitize_audit_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(sanitize_audit_object(map)),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(sanitize_audit_value).collect())
        }
        other => other,
    }
}

fn sanitize_audit_object(map: Map<String, Value>) -> Map<String, Value> {
    map.into_iter()
        .map(|(key, value)| {
            let value = if is_sensitive_audit_key(&key) {
                Value::String("***".to_owned())
            } else {
                sanitize_audit_value(value)
            };

            (key, value)
        })
        .collect()
}

fn is_sensitive_audit_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "password"
            | "old_password"
            | "new_password"
            | "password_hash"
            | "token"
            | "token_hash"
            | "access_token"
            | "refresh_token"
            | "refresh_token_hash"
            | "api_key"
            | "secret"
            | "app_secret"
            | "app_secret_hash"
            | "authorization"
            | "license_key"
            | "license_key_hash"
            | "private_key"
            | "private_key_envelope"
            | "recovery_code"
            | "recovery_codes"
            | "mfa_secret"
    )
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

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("audit log database error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use serde_json::json;

    use super::{sanitize_audit_value, validate_audit_log_query, AuditLogListQuery};

    #[test]
    fn audit_query_rejects_inverted_time_range() {
        let now = Utc::now();
        let query = AuditLogListQuery {
            actor_id: None,
            action: None,
            resource_type: None,
            resource_id: None,
            start_at: Some(now),
            end_at: Some(now - Duration::seconds(1)),
            page: None,
            page_size: None,
        };

        assert!(validate_audit_log_query(&query).is_err());
    }

    #[test]
    fn audit_query_rejects_invalid_action_filter() {
        let query = AuditLogListQuery {
            actor_id: None,
            action: Some("bad action".to_owned()),
            resource_type: None,
            resource_id: None,
            start_at: None,
            end_at: None,
            page: None,
            page_size: None,
        };

        assert!(validate_audit_log_query(&query).is_err());
    }

    #[test]
    fn audit_sanitizer_redacts_sensitive_fields_recursively() {
        let value = json!({
            "password": "secret",
            "profile": {
                "refresh_token": "rt",
                "app_secret": "as",
                "name": "Alice"
            },
            "events": [
                { "authorization": "Bearer token" },
                { "token_hash": "hash" }
            ],
            "revoked_refresh_tokens": 2
        });

        let sanitized = sanitize_audit_value(value);

        assert_eq!(sanitized["password"], "***");
        assert_eq!(sanitized["profile"]["refresh_token"], "***");
        assert_eq!(sanitized["profile"]["app_secret"], "***");
        assert_eq!(sanitized["profile"]["name"], "Alice");
        assert_eq!(sanitized["events"][0]["authorization"], "***");
        assert_eq!(sanitized["events"][1]["token_hash"], "***");
        assert_eq!(sanitized["revoked_refresh_tokens"], 2);
    }
}
