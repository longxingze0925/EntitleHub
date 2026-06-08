use axum::{extract::State, Extension, Json};
use chrono::{DateTime, Utc};
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};

use crate::{
    config::EmailConfig,
    crypto::envelope::{decrypt_bytes, encrypt_bytes, PrivateKeyEnvelope},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const MAX_SMTP_FIELD_LEN: usize = 512;
const MAX_TEST_ERROR_LEN: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct EmailSettingsResponse {
    pub enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_from: String,
    pub smtp_password_configured: bool,
    pub source: String,
    pub last_test_status: Option<String>,
    pub last_test_error: Option<String>,
    pub last_test_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateEmailSettingsRequest {
    pub enabled: bool,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_from: String,
    pub smtp_password: Option<String>,
    #[serde(default)]
    pub clear_password: bool,
}

#[derive(Debug, Deserialize)]
pub struct TestEmailSettingsRequest {
    pub to: String,
    #[serde(default)]
    pub confirm_delivery: bool,
}

#[derive(Debug, Clone)]
pub struct EmailDeliveryConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_from: String,
}

#[derive(Debug, Clone)]
pub struct EmailDeliveryMessage {
    pub to: String,
    pub subject: String,
    pub body: String,
}

#[derive(Debug, Clone, FromRow)]
struct EmailSettingsRecord {
    enabled: bool,
    smtp_host: String,
    smtp_port: i32,
    smtp_user: Option<String>,
    smtp_from: String,
    smtp_password_encrypted: Option<String>,
    last_test_status: Option<String>,
    last_test_error: Option<String>,
    last_test_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
}

struct NormalizedEmailSettings {
    enabled: bool,
    smtp_host: String,
    smtp_port: u16,
    smtp_user: Option<String>,
    smtp_from: String,
    smtp_password_encrypted: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmtpSecurityMode {
    TlsWrapper,
    StartTls,
}

pub async fn get_email_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<EmailSettingsResponse>>, AppError> {
    ensure_admin_permission(&admin, "system:read")?;

    let settings = load_email_settings_response(&state).await?;

    Ok(Json(ApiResponse::ok(settings, request_id.to_string())))
}

pub async fn update_email_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<UpdateEmailSettingsRequest>,
) -> Result<Json<ApiResponse<EmailSettingsResponse>>, AppError> {
    ensure_admin_permission(&admin, "system:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_email_settings_for_update(&mut transaction).await?;
    let input = normalize_update_request(&state, before.as_ref(), payload)?;
    let record = upsert_email_settings(&mut transaction, input).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "email_settings.update",
            resource_type: "email_settings",
            resource_id: None,
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: before.as_ref().map(email_settings_audit_json),
            after_json: Some(email_settings_audit_json(&record)),
            metadata_json: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        EmailSettingsResponse::from_record(record, "database"),
        request_id.to_string(),
    )))
}

pub async fn test_email_settings(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<TestEmailSettingsRequest>,
) -> Result<Json<ApiResponse<EmailSettingsResponse>>, AppError> {
    ensure_admin_permission(&admin, "system:update")?;
    if !payload.confirm_delivery {
        return Err(AppError::validation_failed(
            "email settings delivery test must be confirmed",
        ));
    }
    let to = normalize_required_text("test recipient", &payload.to)?;
    validate_mailbox(&to, "test recipient")?;

    let config = load_email_delivery_config(&state)
        .await
        .map_err(AppError::dependency)?
        .ok_or_else(|| AppError::validation_failed("email delivery is not enabled"))?;

    let result = send_email(
        &config,
        &EmailDeliveryMessage {
            to,
            subject: "EntitleHub 邮件服务测试".to_owned(),
            body: [
                "这是一封 EntitleHub 邮件服务测试邮件。",
                "如果你收到这封邮件，说明 SMTP 配置已经可以正常发送系统邮件。",
            ]
            .join("\n"),
        },
    )
    .await;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let (status, error) = match result {
        Ok(()) => ("success", None),
        Err(error) => ("failed", Some(error)),
    };
    let record =
        update_email_settings_test_result(&mut transaction, status, error.as_deref()).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "email_settings.test",
            resource_type: "email_settings",
            resource_id: None,
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: record.as_ref().map(email_settings_audit_json),
            metadata_json: serde_json::json!({
                "status": status,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    if let Some(error) = error {
        return Err(AppError::dependency(format!(
            "email settings test failed: {error}"
        )));
    }

    let settings = match record {
        Some(record) => EmailSettingsResponse::from_record(record, "database"),
        None => load_email_settings_response(&state).await?,
    };

    Ok(Json(ApiResponse::ok(settings, request_id.to_string())))
}

pub async fn load_email_delivery_config(
    state: &AppState,
) -> Result<Option<EmailDeliveryConfig>, String> {
    let record = find_email_settings(&state.db)
        .await
        .map_err(|error| format!("load email settings failed: {error}"))?;

    if let Some(record) = record {
        if !record.enabled {
            return Ok(None);
        }

        return delivery_config_from_record(state, &record).map(Some);
    }

    delivery_config_from_env(&state.config.email)
}

pub async fn send_email(
    config: &EmailDeliveryConfig,
    email: &EmailDeliveryMessage,
) -> Result<(), String> {
    let from: Mailbox = config
        .smtp_from
        .parse()
        .map_err(|error| format!("SMTP_FROM is invalid: {error}"))?;
    let to: Mailbox = email
        .to
        .parse()
        .map_err(|error| format!("email recipient is invalid: {error}"))?;
    let message = Message::builder()
        .from(from)
        .to(to)
        .subject(&email.subject)
        .body(email.body.clone())
        .map_err(|error| format!("email message build failed: {error}"))?;

    build_smtp_transport(
        &config.smtp_host,
        config.smtp_port,
        smtp_credentials(config.smtp_user.as_deref(), config.smtp_password.as_deref()),
    )?
    .send(message)
    .await
    .map(|_| ())
    .map_err(|error| format!("SMTP send failed: {error}"))
}

async fn load_email_settings_response(state: &AppState) -> Result<EmailSettingsResponse, AppError> {
    match find_email_settings(&state.db).await.map_err(map_db_error)? {
        Some(record) => Ok(EmailSettingsResponse::from_record(record, "database")),
        None => Ok(EmailSettingsResponse::from_env(&state.config.email)),
    }
}

async fn find_email_settings(
    pool: &sqlx::PgPool,
) -> Result<Option<EmailSettingsRecord>, sqlx::Error> {
    sqlx::query_as::<_, EmailSettingsRecord>(
        r#"
        select
          enabled,
          smtp_host,
          smtp_port,
          smtp_user,
          smtp_from,
          smtp_password_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          updated_at
        from email_settings
        where id = true
        "#,
    )
    .fetch_optional(pool)
    .await
}

async fn find_email_settings_for_update(
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<Option<EmailSettingsRecord>, AppError> {
    sqlx::query_as::<_, EmailSettingsRecord>(
        r#"
        select
          enabled,
          smtp_host,
          smtp_port,
          smtp_user,
          smtp_from,
          smtp_password_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          updated_at
        from email_settings
        where id = true
        for update
        "#,
    )
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn upsert_email_settings(
    transaction: &mut Transaction<'_, Postgres>,
    input: NormalizedEmailSettings,
) -> Result<EmailSettingsRecord, AppError> {
    sqlx::query_as::<_, EmailSettingsRecord>(
        r#"
        insert into email_settings (
          id,
          enabled,
          smtp_host,
          smtp_port,
          smtp_user,
          smtp_from,
          smtp_password_encrypted,
          updated_at
        )
        values (true, $1, $2, $3, $4, $5, $6, now())
        on conflict (id)
        do update set
          enabled = excluded.enabled,
          smtp_host = excluded.smtp_host,
          smtp_port = excluded.smtp_port,
          smtp_user = excluded.smtp_user,
          smtp_from = excluded.smtp_from,
          smtp_password_encrypted = excluded.smtp_password_encrypted,
          updated_at = now()
        returning
          enabled,
          smtp_host,
          smtp_port,
          smtp_user,
          smtp_from,
          smtp_password_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          updated_at
        "#,
    )
    .bind(input.enabled)
    .bind(input.smtp_host)
    .bind(input.smtp_port as i32)
    .bind(input.smtp_user)
    .bind(input.smtp_from)
    .bind(input.smtp_password_encrypted)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_email_settings_test_result(
    transaction: &mut Transaction<'_, Postgres>,
    status: &str,
    error: Option<&str>,
) -> Result<Option<EmailSettingsRecord>, AppError> {
    sqlx::query_as::<_, EmailSettingsRecord>(
        r#"
        update email_settings
        set
          last_test_status = $1,
          last_test_error = $2,
          last_test_at = now(),
          updated_at = now()
        where id = true
        returning
          enabled,
          smtp_host,
          smtp_port,
          smtp_user,
          smtp_from,
          smtp_password_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          updated_at
        "#,
    )
    .bind(status)
    .bind(error.map(truncate_test_error))
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn normalize_update_request(
    state: &AppState,
    before: Option<&EmailSettingsRecord>,
    payload: UpdateEmailSettingsRequest,
) -> Result<NormalizedEmailSettings, AppError> {
    let smtp_host = normalize_text("SMTP 主机", &payload.smtp_host)?;
    let smtp_user = payload
        .smtp_user
        .as_deref()
        .map(|value| normalize_text("SMTP 用户名", value))
        .transpose()?;
    let smtp_from = normalize_text("发件邮箱", &payload.smtp_from)?;
    if !smtp_from.is_empty() {
        validate_mailbox(&smtp_from, "发件邮箱")?;
    }

    let password = payload
        .smtp_password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let smtp_password_encrypted = if smtp_user.is_none() {
        if password.is_some() {
            return Err(AppError::validation_failed(
                "填写 SMTP 密码时必须填写 SMTP 用户名",
            ));
        }
        None
    } else {
        match (payload.clear_password, password) {
            (true, _) => None,
            (false, Some(password)) => Some(encrypt_secret_to_text(state, password)?),
            (false, None) => before.and_then(|record| record.smtp_password_encrypted.clone()),
        }
    };

    if payload.enabled {
        if smtp_host.is_empty() || smtp_from.is_empty() {
            return Err(AppError::validation_failed(
                "enabled email settings require SMTP host and from address",
            ));
        }
        if smtp_user.is_some() && smtp_password_encrypted.is_none() {
            return Err(AppError::validation_failed("SMTP 密码或授权码不能为空"));
        }
    }

    Ok(NormalizedEmailSettings {
        enabled: payload.enabled,
        smtp_host,
        smtp_port: payload.smtp_port,
        smtp_user,
        smtp_from,
        smtp_password_encrypted,
    })
}

fn delivery_config_from_record(
    state: &AppState,
    record: &EmailSettingsRecord,
) -> Result<EmailDeliveryConfig, String> {
    let smtp_password = record
        .smtp_password_encrypted
        .as_deref()
        .map(|value| decrypt_secret_text(state, value))
        .transpose()?;
    let config = EmailDeliveryConfig {
        smtp_host: record.smtp_host.clone(),
        smtp_port: record.smtp_port as u16,
        smtp_user: record.smtp_user.clone(),
        smtp_password,
        smtp_from: record.smtp_from.clone(),
    };
    validate_delivery_config(&config)?;

    Ok(config)
}

fn delivery_config_from_env(config: &EmailConfig) -> Result<Option<EmailDeliveryConfig>, String> {
    if !config.outbox_worker_enabled {
        return Ok(None);
    }
    let delivery_config = EmailDeliveryConfig {
        smtp_host: config
            .smtp_host
            .clone()
            .ok_or_else(|| "SMTP_HOST is not configured".to_owned())?,
        smtp_port: config.smtp_port,
        smtp_user: config.smtp_user.clone(),
        smtp_password: config.smtp_password.clone(),
        smtp_from: config
            .smtp_from
            .clone()
            .ok_or_else(|| "SMTP_FROM is not configured".to_owned())?,
    };
    validate_delivery_config(&delivery_config)?;

    Ok(Some(delivery_config))
}

fn validate_delivery_config(config: &EmailDeliveryConfig) -> Result<(), String> {
    if config.smtp_host.trim().is_empty() {
        return Err("SMTP host is not configured".to_owned());
    }
    if config.smtp_from.trim().is_empty() {
        return Err("SMTP from is not configured".to_owned());
    }
    if config.smtp_user.is_some() != config.smtp_password.is_some() {
        return Err("SMTP user and password must be configured together".to_owned());
    }
    validate_mailbox(&config.smtp_from, "SMTP from").map_err(|error| error.to_string())?;

    Ok(())
}

pub fn build_smtp_transport(
    smtp_host: &str,
    smtp_port: u16,
    credentials: Option<(&str, &str)>,
) -> Result<AsyncSmtpTransport<Tokio1Executor>, String> {
    let mut builder = match smtp_security_mode(smtp_port) {
        SmtpSecurityMode::TlsWrapper => AsyncSmtpTransport::<Tokio1Executor>::relay(smtp_host),
        SmtpSecurityMode::StartTls => {
            AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(smtp_host)
        }
    }
    .map_err(|error| format!("SMTP relay config failed: {error}"))?
    .port(smtp_port);

    if let Some((user, password)) = credentials {
        builder = builder.credentials(Credentials::new(user.to_owned(), password.to_owned()));
    }

    Ok(builder.build())
}

fn smtp_security_mode(smtp_port: u16) -> SmtpSecurityMode {
    if smtp_port == 465 {
        SmtpSecurityMode::TlsWrapper
    } else {
        SmtpSecurityMode::StartTls
    }
}

pub fn smtp_credentials<'a>(
    user: Option<&'a str>,
    password: Option<&'a str>,
) -> Option<(&'a str, &'a str)> {
    match (user, password) {
        (Some(user), Some(password)) => Some((user, password)),
        _ => None,
    }
}

fn encrypt_secret_to_text(state: &AppState, value: &str) -> Result<String, AppError> {
    let envelope = encrypt_bytes(&state.config.security.master_key, value.as_bytes())?;
    serde_json::to_string(&envelope).map_err(|error| {
        AppError::crypto(format!("email settings secret serialize failed: {error}"))
    })
}

fn decrypt_secret_text(state: &AppState, value: &str) -> Result<String, String> {
    let envelope: PrivateKeyEnvelope = serde_json::from_str(value)
        .map_err(|error| format!("email settings secret envelope invalid: {error}"))?;
    let bytes = decrypt_bytes(&state.config.security.master_key, &envelope)
        .map_err(|error| format!("email settings secret decrypt failed: {error}"))?;
    String::from_utf8(bytes).map_err(|error| format!("email settings secret is not utf8: {error}"))
}

fn normalize_required_text(label: &str, value: &str) -> Result<String, AppError> {
    let value = normalize_text(label, value)?;
    if value.is_empty() {
        return Err(AppError::validation_failed(format!("{label}不能为空")));
    }

    Ok(value)
}

fn normalize_text(label: &str, value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.len() > MAX_SMTP_FIELD_LEN {
        return Err(AppError::validation_failed(format!("{label}过长")));
    }

    Ok(value.to_owned())
}

fn validate_mailbox(value: &str, label: &str) -> Result<(), AppError> {
    value
        .parse::<Mailbox>()
        .map(|_| ())
        .map_err(|error| AppError::validation_failed(format!("{label}格式不正确：{error}")))
}

fn email_settings_audit_json(record: &EmailSettingsRecord) -> serde_json::Value {
    serde_json::json!({
        "enabled": record.enabled,
        "smtp_host": record.smtp_host,
        "smtp_port": record.smtp_port,
        "smtp_user": record.smtp_user,
        "smtp_from": record.smtp_from,
        "smtp_password_configured": record.smtp_password_encrypted.is_some(),
        "last_test_status": record.last_test_status,
        "last_test_error": record.last_test_error,
        "last_test_at": record.last_test_at,
        "updated_at": record.updated_at,
    })
}

fn truncate_test_error(error: &str) -> String {
    if error.len() <= MAX_TEST_ERROR_LEN {
        return error.to_owned();
    }

    error.chars().take(MAX_TEST_ERROR_LEN).collect()
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
    AppError::dependency(format!("email settings database error: {error}"))
}

impl EmailSettingsResponse {
    fn from_record(record: EmailSettingsRecord, source: &str) -> Self {
        Self {
            enabled: record.enabled,
            smtp_host: record.smtp_host,
            smtp_port: record.smtp_port as u16,
            smtp_user: record.smtp_user,
            smtp_from: record.smtp_from,
            smtp_password_configured: record.smtp_password_encrypted.is_some(),
            source: source.to_owned(),
            last_test_status: record.last_test_status,
            last_test_error: record.last_test_error,
            last_test_at: record.last_test_at,
            updated_at: Some(record.updated_at),
        }
    }

    fn from_env(config: &EmailConfig) -> Self {
        Self {
            enabled: config.outbox_worker_enabled,
            smtp_host: config.smtp_host.clone().unwrap_or_default(),
            smtp_port: config.smtp_port,
            smtp_user: config.smtp_user.clone(),
            smtp_from: config.smtp_from.clone().unwrap_or_default(),
            smtp_password_configured: config.smtp_password.is_some(),
            source: "environment".to_owned(),
            last_test_status: None,
            last_test_error: None,
            last_test_at: None,
            updated_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        smtp_credentials, smtp_security_mode, validate_delivery_config, EmailDeliveryConfig,
        SmtpSecurityMode,
    };

    #[test]
    fn delivery_config_requires_user_and_password_together() {
        let config = EmailDeliveryConfig {
            smtp_host: "smtp.example.com".to_owned(),
            smtp_port: 587,
            smtp_user: Some("user@example.com".to_owned()),
            smtp_password: None,
            smtp_from: "user@example.com".to_owned(),
        };

        assert!(validate_delivery_config(&config).is_err());
    }

    #[test]
    fn delivery_config_accepts_authenticated_smtp() {
        let config = EmailDeliveryConfig {
            smtp_host: "smtp.example.com".to_owned(),
            smtp_port: 587,
            smtp_user: Some("user@example.com".to_owned()),
            smtp_password: Some("secret".to_owned()),
            smtp_from: "user@example.com".to_owned(),
        };

        assert!(validate_delivery_config(&config).is_ok());
    }

    #[test]
    fn smtp_security_mode_uses_tls_wrapper_for_465() {
        assert_eq!(smtp_security_mode(465), SmtpSecurityMode::TlsWrapper);
    }

    #[test]
    fn smtp_security_mode_uses_starttls_for_587_and_custom_ports() {
        assert_eq!(smtp_security_mode(587), SmtpSecurityMode::StartTls);
        assert_eq!(smtp_security_mode(2525), SmtpSecurityMode::StartTls);
    }

    #[test]
    fn smtp_credentials_require_user_and_password() {
        assert_eq!(
            smtp_credentials(Some("user@example.com"), Some("secret")),
            Some(("user@example.com", "secret"))
        );
        assert_eq!(smtp_credentials(Some("user@example.com"), None), None);
        assert_eq!(smtp_credentials(None, Some("secret")), None);
    }
}
