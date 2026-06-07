use axum::{
    extract::{Path, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const MAX_SETTING_KEY_LEN: usize = 128;
const MAX_SETTING_VALUE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SystemSetting {
    pub key: String,
    pub value: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct SystemSettingListResponse {
    pub items: Vec<SystemSetting>,
}

#[derive(Debug, Serialize)]
pub struct SystemSettingResponse {
    pub setting: SystemSetting,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSystemSettingRequest {
    pub value: Value,
}

pub async fn list_system_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<SystemSettingListResponse>>, AppError> {
    ensure_admin_permission(&admin, "system:read")?;

    let settings = list_settings(&state).await?;

    Ok(Json(ApiResponse::ok(
        SystemSettingListResponse { items: settings },
        request_id.to_string(),
    )))
}

pub async fn update_system_setting(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(key): Path<String>,
    Json(payload): Json<UpdateSystemSettingRequest>,
) -> Result<Json<ApiResponse<SystemSettingResponse>>, AppError> {
    ensure_admin_permission(&admin, "system:update")?;
    let key = normalize_setting_key(&key)?;
    validate_setting_value(&payload.value)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_setting_for_update(&mut transaction, &key).await?;
    let setting = upsert_setting(&mut transaction, &key, payload.value).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "system_setting.update",
            resource_type: "system_setting",
            resource_id: None,
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: before.as_ref().map(setting_audit_json),
            after_json: Some(setting_audit_json(&setting)),
            metadata_json: json!({
                "key": &setting.key,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        SystemSettingResponse { setting },
        request_id.to_string(),
    )))
}

async fn list_settings(state: &AppState) -> Result<Vec<SystemSetting>, AppError> {
    sqlx::query_as::<_, SystemSetting>(
        r#"
        select
          key,
          value,
          updated_at
        from system_settings
        order by key asc
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_setting_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    key: &str,
) -> Result<Option<SystemSetting>, AppError> {
    sqlx::query_as::<_, SystemSetting>(
        r#"
        select
          key,
          value,
          updated_at
        from system_settings
        where key = $1
        for update
        "#,
    )
    .bind(key)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn upsert_setting(
    transaction: &mut Transaction<'_, Postgres>,
    key: &str,
    value: Value,
) -> Result<SystemSetting, AppError> {
    sqlx::query_as::<_, SystemSetting>(
        r#"
        insert into system_settings (
          key,
          value,
          updated_at
        )
        values ($1, $2, now())
        on conflict (key)
        do update set
          value = excluded.value,
          updated_at = now()
        returning
          key,
          value,
          updated_at
        "#,
    )
    .bind(key)
    .bind(value)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn setting_audit_json(setting: &SystemSetting) -> Value {
    json!({
        "key": &setting.key,
        "value": &setting.value,
        "updated_at": &setting.updated_at,
    })
}

fn normalize_setting_key(key: &str) -> Result<String, AppError> {
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() || key.len() > MAX_SETTING_KEY_LEN {
        return Err(AppError::validation_failed("setting key is invalid"));
    }
    if !key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Err(AppError::validation_failed("setting key is invalid"));
    }
    if contains_sensitive_setting_key_part(&key) {
        return Err(AppError::validation_failed(
            "sensitive settings must stay in environment variables",
        ));
    }

    Ok(key)
}

fn contains_sensitive_setting_key_part(key: &str) -> bool {
    const SENSITIVE_PARTS: &[&str] = &[
        "secret",
        "password",
        "token",
        "key",
        "private",
        "credential",
        "credentials",
        "cert",
        "certificate",
    ];

    key.split(['.', '_', '-', ':'])
        .filter(|part| !part.is_empty())
        .any(|part| SENSITIVE_PARTS.contains(&part))
}

fn validate_setting_value(value: &Value) -> Result<(), AppError> {
    if value.is_null() {
        return Err(AppError::validation_failed("setting value cannot be null"));
    }

    let len = serde_json::to_vec(value)
        .map_err(|error| AppError::validation_failed(format!("setting value invalid: {error}")))?
        .len();
    if len > MAX_SETTING_VALUE_BYTES {
        return Err(AppError::validation_failed("setting value is too large"));
    }

    Ok(())
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
    AppError::dependency(format!("system settings database error: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use crate::modules::auth::session::AdminContext;

    use super::{ensure_admin_permission, normalize_setting_key, validate_setting_value};

    #[test]
    fn setting_key_normalizes_and_rejects_sensitive_names() {
        assert_eq!(
            normalize_setting_key(" Billing.Mode ").expect("key"),
            "billing.mode"
        );
        assert!(normalize_setting_key("smtp.password").is_err());
        assert!(normalize_setting_key("master_key").is_err());
        assert!(normalize_setting_key("jwt.signing_key").is_err());
        assert!(normalize_setting_key("private-key").is_err());
        assert!(normalize_setting_key("api.credential").is_err());
        assert!(normalize_setting_key("monkey.mode").is_ok());
        assert!(normalize_setting_key("bad key").is_err());
    }

    #[test]
    fn setting_value_rejects_null() {
        assert!(validate_setting_value(&json!({"enabled": true})).is_ok());
        assert!(validate_setting_value(&json!(null)).is_err());
    }

    #[test]
    fn permission_check_uses_system_permissions() {
        let mut admin = AdminContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            team_member_id: Uuid::nil(),
            email: "admin@example.com".to_owned(),
            name: "Admin".to_owned(),
            email_verified: true,
            mfa_enabled: false,
            tenant_name: "Default".to_owned(),
            roles: vec!["admin".to_owned()],
            permissions: vec!["system:read".to_owned()],
        };

        assert!(ensure_admin_permission(&admin, "system:read").is_ok());
        assert!(ensure_admin_permission(&admin, "system:update").is_err());
        admin.permissions.push("system:update".to_owned());
        assert!(ensure_admin_permission(&admin, "system:update").is_ok());
    }
}
