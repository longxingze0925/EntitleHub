use axum::{
    body::Bytes,
    extract::{Path, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use std::time::Instant;
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, encrypt_bytes, PrivateKeyEnvelope},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    metrics::{record_notification_delivery, NotificationDeliveryStatus},
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const MAX_CHANNEL_NAME_LEN: usize = 128;
const MAX_CONFIG_BYTES: usize = 16 * 1024;
const MAX_SECRET_BYTES: usize = 16 * 1024;
const MAX_TEST_ERROR_LEN: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct NotificationChannel {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub config: Value,
    pub secret_configured: bool,
    pub last_test_status: Option<String>,
    pub last_test_error: Option<String>,
    pub last_test_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct NotificationChannelListResponse {
    pub items: Vec<NotificationChannel>,
}

#[derive(Debug, Serialize)]
pub struct NotificationChannelResponse {
    pub channel: NotificationChannel,
}

#[derive(Debug, Deserialize)]
pub struct CreateNotificationChannelRequest {
    pub name: String,
    pub kind: String,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub config: Value,
    pub secret: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNotificationChannelRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<Value>,
    pub secret: Option<Value>,
    #[serde(default)]
    pub clear_secret: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct TestNotificationChannelRequest {
    pub mode: Option<String>,
    #[serde(default)]
    pub confirm_delivery: bool,
}

#[derive(Debug, Clone, FromRow)]
struct NotificationChannelRecord {
    id: Uuid,
    name: String,
    kind: String,
    enabled: bool,
    config_json: Value,
    secret_encrypted: Option<String>,
    last_test_status: Option<String>,
    last_test_error: Option<String>,
    last_test_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

struct CreateChannelInput {
    name: String,
    kind: String,
    enabled: bool,
    config: Value,
    secret_encrypted: Option<String>,
}

struct UpdateChannelInput {
    name: String,
    enabled: bool,
    config: Value,
    secret_encrypted: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NotificationChannelTestMode {
    DryRun,
    Delivery,
}

impl NotificationChannelTestMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::DryRun => "dry_run",
            Self::Delivery => "delivery",
        }
    }
}

struct TestDeliveryMessage {
    summary: String,
    severity: String,
    source: String,
    body: String,
    payload: Value,
}

pub async fn list_notification_channels(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<NotificationChannelListResponse>>, AppError> {
    ensure_admin_permission(&admin, "notification:read")?;

    let items = list_channels(&state, admin.tenant_id)
        .await?
        .into_iter()
        .map(NotificationChannel::from)
        .collect();

    Ok(Json(ApiResponse::ok(
        NotificationChannelListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn create_notification_channel(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateNotificationChannelRequest>,
) -> Result<Json<ApiResponse<NotificationChannelResponse>>, AppError> {
    ensure_admin_permission(&admin, "notification:update")?;
    let input = normalize_create_input(&state, payload)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let record = insert_channel(&mut transaction, admin.tenant_id, input).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "notification_channel.create",
            resource_type: "notification_channel",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(notification_channel_audit_json(&record)),
            metadata_json: json!({
                "name": &record.name,
                "kind": &record.kind,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        NotificationChannelResponse {
            channel: NotificationChannel::from(record),
        },
        request_id.to_string(),
    )))
}

pub async fn update_notification_channel(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(channel_id): Path<Uuid>,
    Json(payload): Json<UpdateNotificationChannelRequest>,
) -> Result<Json<ApiResponse<NotificationChannelResponse>>, AppError> {
    ensure_admin_permission(&admin, "notification:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_channel_for_update(&mut transaction, admin.tenant_id, channel_id).await?;
    let input = normalize_update_input(&state, &before, payload)?;
    let record =
        update_channel_in_transaction(&mut transaction, admin.tenant_id, channel_id, input).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "notification_channel.update",
            resource_type: "notification_channel",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(notification_channel_audit_json(&before)),
            after_json: Some(notification_channel_audit_json(&record)),
            metadata_json: json!({
                "name": &record.name,
                "kind": &record.kind,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        NotificationChannelResponse {
            channel: NotificationChannel::from(record),
        },
        request_id.to_string(),
    )))
}

pub async fn test_notification_channel(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(channel_id): Path<Uuid>,
    body: Bytes,
) -> Result<Json<ApiResponse<NotificationChannelResponse>>, AppError> {
    ensure_admin_permission(&admin, "notification:update")?;
    let payload = parse_test_payload(&body)?;
    let mode = normalize_test_mode(&payload)?;

    let before = find_channel(&state, admin.tenant_id, channel_id).await?;
    let test_result = match mode {
        NotificationChannelTestMode::DryRun => run_dry_run_test(&state, &before),
        NotificationChannelTestMode::Delivery => run_delivery_test(&state, &before).await,
    };
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let (status, error) = match &test_result {
        Ok(()) => ("success", None),
        Err(error) => ("failed", Some(error.to_string())),
    };
    let record = update_test_result_in_transaction(
        &mut transaction,
        admin.tenant_id,
        channel_id,
        status,
        error.as_deref(),
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "notification_channel.test",
            resource_type: "notification_channel",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(notification_channel_audit_json(&before)),
            after_json: Some(notification_channel_audit_json(&record)),
            metadata_json: json!({
                "name": &record.name,
                "kind": &record.kind,
                "dry_run": mode == NotificationChannelTestMode::DryRun,
                "mode": mode.as_str(),
                "status": status,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    if let Err(error) = test_result {
        return Err(error);
    }

    Ok(Json(ApiResponse::ok(
        NotificationChannelResponse {
            channel: NotificationChannel::from(record),
        },
        request_id.to_string(),
    )))
}

async fn find_channel(
    state: &AppState,
    tenant_id: Uuid,
    channel_id: Uuid,
) -> Result<NotificationChannelRecord, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(&notification_channel_select_sql(
        "where tenant_id = $1 and id = $2",
    ))
    .bind(tenant_id)
    .bind(channel_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("notification channel not found"))
}

async fn list_channels(
    state: &AppState,
    tenant_id: Uuid,
) -> Result<Vec<NotificationChannelRecord>, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(&notification_channel_select_sql(
        "where tenant_id = $1 order by created_at desc, id desc",
    ))
    .bind(tenant_id)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn insert_channel(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    input: CreateChannelInput,
) -> Result<NotificationChannelRecord, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(
        r#"
        insert into notification_channels (
          tenant_id,
          name,
          kind,
          enabled,
          config_json,
          secret_encrypted
        )
        values ($1, $2, $3, $4, $5, $6)
        returning
          id,
          name,
          kind,
          enabled,
          config_json,
          secret_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(input.name)
    .bind(input.kind)
    .bind(input.enabled)
    .bind(input.config)
    .bind(input.secret_encrypted)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn find_channel_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    channel_id: Uuid,
) -> Result<NotificationChannelRecord, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(&notification_channel_select_sql(
        "where tenant_id = $1 and id = $2 for update",
    ))
    .bind(tenant_id)
    .bind(channel_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("notification channel not found"))
}

async fn update_channel_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    channel_id: Uuid,
    input: UpdateChannelInput,
) -> Result<NotificationChannelRecord, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(
        r#"
        update notification_channels
        set
          name = $3,
          enabled = $4,
          config_json = $5,
          secret_encrypted = $6,
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          name,
          kind,
          enabled,
          config_json,
          secret_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(channel_id)
    .bind(input.name)
    .bind(input.enabled)
    .bind(input.config)
    .bind(input.secret_encrypted)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_test_result_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    channel_id: Uuid,
    status: &str,
    error: Option<&str>,
) -> Result<NotificationChannelRecord, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(
        r#"
        update notification_channels
        set
          last_test_status = $3,
          last_test_error = $4,
          last_test_at = now(),
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          name,
          kind,
          enabled,
          config_json,
          secret_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(channel_id)
    .bind(status)
    .bind(error.map(truncate_test_error))
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn notification_channel_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
          id,
          name,
          kind,
          enabled,
          config_json,
          secret_encrypted,
          last_test_status,
          last_test_error,
          last_test_at,
          created_at,
          updated_at
        from notification_channels
        {where_clause}
        "#
    )
}

fn normalize_create_input(
    state: &AppState,
    payload: CreateNotificationChannelRequest,
) -> Result<CreateChannelInput, AppError> {
    let kind = normalize_kind(&payload.kind)?;
    let name = normalize_channel_name(&payload.name)?;
    let mut config = normalize_config(payload.config)?;
    let secret = payload
        .secret
        .map(normalize_secret)
        .transpose()?
        .filter(|value| !value.as_object().is_some_and(|map| map.is_empty()));

    apply_public_summary(&kind, &mut config, secret.as_ref())?;
    validate_kind_config(&kind, &config)?;
    let secret_encrypted = secret
        .as_ref()
        .map(|secret| encrypt_secret_to_text(state, secret))
        .transpose()?;

    Ok(CreateChannelInput {
        name,
        kind,
        enabled: payload.enabled.unwrap_or(true),
        config,
        secret_encrypted,
    })
}

fn normalize_update_input(
    state: &AppState,
    before: &NotificationChannelRecord,
    payload: UpdateNotificationChannelRequest,
) -> Result<UpdateChannelInput, AppError> {
    let name = match payload.name {
        Some(name) => normalize_channel_name(&name)?,
        None => before.name.clone(),
    };
    let mut config = match payload.config {
        Some(config) => normalize_config(config)?,
        None => before.config_json.clone(),
    };

    let new_secret = payload
        .secret
        .map(normalize_secret)
        .transpose()?
        .filter(|value| !value.as_object().is_some_and(|map| map.is_empty()));
    apply_public_summary(&before.kind, &mut config, new_secret.as_ref())?;
    validate_kind_config(&before.kind, &config)?;

    let secret_encrypted = match (payload.clear_secret, new_secret) {
        (true, _) => None,
        (false, Some(secret)) => Some(encrypt_secret_to_text(state, &secret)?),
        (false, None) => before.secret_encrypted.clone(),
    };

    Ok(UpdateChannelInput {
        name,
        enabled: payload.enabled.unwrap_or(before.enabled),
        config,
        secret_encrypted,
    })
}

fn normalize_channel_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.len() > MAX_CHANNEL_NAME_LEN {
        return Err(AppError::validation_failed(
            "notification channel name is invalid",
        ));
    }

    Ok(name.to_owned())
}

fn normalize_kind(kind: &str) -> Result<String, AppError> {
    let kind = kind.trim().to_ascii_lowercase();
    match kind.as_str() {
        "webhook" | "email" | "pagerduty" => Ok(kind),
        _ => Err(AppError::validation_failed(
            "notification channel kind is invalid",
        )),
    }
}

fn parse_test_payload(body: &Bytes) -> Result<TestNotificationChannelRequest, AppError> {
    if body.is_empty() {
        return Ok(TestNotificationChannelRequest::default());
    }

    serde_json::from_slice(body).map_err(|error| {
        AppError::validation_failed(format!(
            "notification channel test payload invalid: {error}"
        ))
    })
}

fn normalize_test_mode(
    payload: &TestNotificationChannelRequest,
) -> Result<NotificationChannelTestMode, AppError> {
    let mode = payload
        .mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("dry_run")
        .to_ascii_lowercase();

    match mode.as_str() {
        "dry_run" | "dry-run" | "dryrun" => Ok(NotificationChannelTestMode::DryRun),
        "delivery" | "send" => {
            if payload.confirm_delivery {
                Ok(NotificationChannelTestMode::Delivery)
            } else {
                Err(AppError::validation_failed(
                    "notification channel delivery test must be confirmed",
                ))
            }
        }
        _ => Err(AppError::validation_failed(
            "notification channel test mode is invalid",
        )),
    }
}

fn normalize_config(value: Value) -> Result<Value, AppError> {
    let value = if value.is_null() { json!({}) } else { value };
    validate_json_object("notification channel config", &value)?;
    reject_sensitive_public_keys(&value)?;
    ensure_json_size("notification channel config", &value, MAX_CONFIG_BYTES)?;

    Ok(value)
}

fn normalize_secret(value: Value) -> Result<Value, AppError> {
    let value = if value.is_null() { json!({}) } else { value };
    validate_json_object("notification channel secret", &value)?;
    ensure_json_size("notification channel secret", &value, MAX_SECRET_BYTES)?;

    Ok(value)
}

fn validate_json_object(label: &str, value: &Value) -> Result<(), AppError> {
    if value.is_object() {
        return Ok(());
    }

    Err(AppError::validation_failed(format!(
        "{label} must be an object"
    )))
}

fn ensure_json_size(label: &str, value: &Value, max_bytes: usize) -> Result<(), AppError> {
    let len = serde_json::to_vec(value)
        .map_err(|error| AppError::validation_failed(format!("{label} invalid: {error}")))?
        .len();
    if len > max_bytes {
        return Err(AppError::validation_failed(format!("{label} is too large")));
    }

    Ok(())
}

fn reject_sensitive_public_keys(value: &Value) -> Result<(), AppError> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let lower = key.to_ascii_lowercase();
                if ["secret", "password", "token", "key", "url"]
                    .iter()
                    .any(|part| lower.contains(part))
                {
                    return Err(AppError::validation_failed(
                        "sensitive notification config must be submitted as secret",
                    ));
                }
                reject_sensitive_public_keys(value)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                reject_sensitive_public_keys(item)?;
            }
        }
        _ => {}
    }

    Ok(())
}

fn validate_kind_config(kind: &str, config: &Value) -> Result<(), AppError> {
    match kind {
        "email" => {
            require_text_config(config, "smtp_host")?;
            require_number_config(config, "smtp_port")?;
            require_text_config(config, "from")?;
            require_text_or_array_config(config, "to")?;
        }
        "pagerduty" => {
            let _ = optional_text_config(config, "service")?;
        }
        "webhook" => {}
        _ => {
            return Err(AppError::validation_failed(
                "notification channel kind is invalid",
            ))
        }
    }

    Ok(())
}

fn run_dry_run_test(state: &AppState, channel: &NotificationChannelRecord) -> Result<(), AppError> {
    if !channel.enabled {
        return Err(AppError::validation_failed(
            "notification channel is disabled",
        ));
    }
    let secret = decrypt_channel_secret(state, channel)?;

    match channel.kind.as_str() {
        "webhook" => validate_webhook_secret(&secret),
        "email" => validate_email_channel(&channel.config_json, &secret),
        "pagerduty" => validate_pagerduty_secret(&secret),
        _ => Err(AppError::validation_failed(
            "notification channel kind is invalid",
        )),
    }
}

async fn run_delivery_test(
    state: &AppState,
    channel: &NotificationChannelRecord,
) -> Result<(), AppError> {
    run_dry_run_test(state, channel)?;
    let secret = decrypt_channel_secret(state, channel)?;
    let message = build_test_delivery_message(channel);
    let client = reqwest::Client::builder()
        .timeout(state.config.alerting.delivery_timeout)
        .build()
        .map_err(|error| {
            AppError::dependency(format!("notification test client failed: {error}"))
        })?;
    let started_at = Instant::now();
    let result = match channel.kind.as_str() {
        "webhook" => deliver_webhook_test(&client, channel, &secret, &message).await,
        "email" => deliver_email_test(channel, &secret, &message).await,
        "pagerduty" => deliver_pagerduty_test(&client, channel, &secret, &message).await,
        _ => Err("notification channel kind is invalid".to_owned()),
    };
    let delivery_status = if result.is_ok() {
        NotificationDeliveryStatus::Success
    } else {
        NotificationDeliveryStatus::Failure
    };
    record_notification_delivery(&channel.kind, delivery_status, started_at.elapsed());

    result.map_err(|error| {
        AppError::dependency(format!("notification test delivery failed: {error}"))
    })
}

fn validate_webhook_secret(secret: &Value) -> Result<(), AppError> {
    let url = require_text_secret(secret, "url")
        .or_else(|_| require_text_secret(secret, "webhook_url"))?;
    validate_http_url(url, "webhook url")
}

fn validate_email_channel(config: &Value, secret: &Value) -> Result<(), AppError> {
    validate_kind_config("email", config)?;
    require_text_secret(secret, "smtp_password")?;
    let user = optional_text_config(config, "smtp_user")?;
    if user.is_none() {
        require_text_secret(secret, "smtp_user")?;
    }

    Ok(())
}

fn validate_pagerduty_secret(secret: &Value) -> Result<(), AppError> {
    require_text_secret(secret, "routing_key")?;

    Ok(())
}

fn build_test_delivery_message(channel: &NotificationChannelRecord) -> TestDeliveryMessage {
    let summary = format!("Notification channel test: {}", channel.name);
    let severity = "info".to_owned();
    let source = "admin-notification-channel-test".to_owned();
    let body = [
        "Status: firing".to_owned(),
        format!("Severity: {severity}"),
        format!("Summary: {summary}"),
        format!("Source: {source}"),
        format!("Channel: {} ({})", channel.name, channel.kind),
    ]
    .join("\n");
    let payload = json!({
        "status": "firing",
        "receiver": "admin-notification-channel-test",
        "groupKey": format!("admin-test:{}", channel.id),
        "commonLabels": {
            "alertname": "NotificationChannelTest",
            "severity": &severity,
            "channel_kind": &channel.kind,
        },
        "commonAnnotations": {
            "summary": &summary,
        },
        "externalURL": "admin",
        "alerts": [{
            "status": "firing",
            "labels": {
                "alertname": "NotificationChannelTest",
                "severity": &severity,
                "channel_kind": &channel.kind,
            },
            "annotations": {
                "summary": &summary,
            },
            "startsAt": Utc::now().to_rfc3339(),
            "generatorURL": "admin",
            "fingerprint": channel.id.to_string(),
        }],
    });

    TestDeliveryMessage {
        summary,
        severity,
        source,
        body,
        payload,
    }
}

async fn deliver_webhook_test(
    client: &reqwest::Client,
    channel: &NotificationChannelRecord,
    secret: &Value,
    message: &TestDeliveryMessage,
) -> Result<(), String> {
    let url = text_field(secret, "url")
        .or_else(|| text_field(secret, "webhook_url"))
        .ok_or_else(|| "webhook url is not configured".to_owned())?;
    validate_http_url(url, "webhook url").map_err(|error| error.to_string())?;
    let response = client
        .post(url)
        .header("user-agent", "user-admin-notification-channel-test/1")
        .json(&json!({
            "schema_version": 1,
            "source": "admin-test",
            "channel_id": channel.id,
            "channel_name": channel.name,
            "summary": message.summary,
            "severity": message.severity,
            "alert_source": message.source,
            "payload": message.payload,
        }))
        .send()
        .await
        .map_err(|error| format!("webhook send failed: {error}"))?;

    ensure_success_status(response.status(), "webhook")
}

async fn deliver_email_test(
    channel: &NotificationChannelRecord,
    secret: &Value,
    message: &TestDeliveryMessage,
) -> Result<(), String> {
    let host = text_field(&channel.config_json, "smtp_host")
        .ok_or_else(|| "smtp_host is not configured".to_owned())?;
    let port = number_field(&channel.config_json, "smtp_port").unwrap_or(587);
    let from = text_field(&channel.config_json, "from")
        .ok_or_else(|| "email from is not configured".to_owned())?;
    let recipients = recipients_field(&channel.config_json, "to")?;
    let user =
        text_field(&channel.config_json, "smtp_user").or_else(|| text_field(secret, "smtp_user"));
    let password = text_field(secret, "smtp_password")
        .ok_or_else(|| "smtp_password is not configured".to_owned())?;

    let mut builder = AsyncSmtpTransport::<Tokio1Executor>::relay(host)
        .map_err(|error| format!("smtp relay config failed: {error}"))?
        .port(port);
    if let Some(user) = user {
        builder = builder.credentials(Credentials::new(user.to_owned(), password.to_owned()));
    }
    let mailer = builder.build();
    let from: Mailbox = from
        .parse()
        .map_err(|error| format!("email from is invalid: {error}"))?;
    let mut message_builder = Message::builder()
        .from(from)
        .subject(format!("[{}] {}", message.severity, message.summary));
    for recipient in recipients {
        let recipient = recipient
            .parse::<Mailbox>()
            .map_err(|error| format!("email recipient is invalid: {error}"))?;
        message_builder = message_builder.to(recipient);
    }
    let email = message_builder
        .body(message.body.clone())
        .map_err(|error| format!("email message build failed: {error}"))?;

    mailer
        .send(email)
        .await
        .map(|_| ())
        .map_err(|error| format!("smtp send failed: {error}"))
}

async fn deliver_pagerduty_test(
    client: &reqwest::Client,
    channel: &NotificationChannelRecord,
    secret: &Value,
    message: &TestDeliveryMessage,
) -> Result<(), String> {
    let routing_key = text_field(secret, "routing_key")
        .ok_or_else(|| "pagerduty routing_key is not configured".to_owned())?;
    let dedup_key = format!("admin-notification-channel-test:{}", channel.id);
    let trigger = build_pagerduty_test_event(channel, routing_key, &dedup_key, message, "trigger");
    let response = client
        .post("https://events.pagerduty.com/v2/enqueue")
        .header("user-agent", "user-admin-notification-channel-test/1")
        .json(&trigger)
        .send()
        .await
        .map_err(|error| format!("pagerduty trigger failed: {error}"))?;
    ensure_success_status(response.status(), "pagerduty trigger")?;

    let resolve = build_pagerduty_test_event(channel, routing_key, &dedup_key, message, "resolve");
    let response = client
        .post("https://events.pagerduty.com/v2/enqueue")
        .header("user-agent", "user-admin-notification-channel-test/1")
        .json(&resolve)
        .send()
        .await
        .map_err(|error| format!("pagerduty resolve failed: {error}"))?;

    ensure_success_status(response.status(), "pagerduty resolve")
}

fn build_pagerduty_test_event(
    channel: &NotificationChannelRecord,
    routing_key: &str,
    dedup_key: &str,
    message: &TestDeliveryMessage,
    action: &str,
) -> Value {
    json!({
        "routing_key": routing_key,
        "event_action": action,
        "dedup_key": dedup_key,
        "payload": {
            "summary": message.summary,
            "source": message.source,
            "severity": "info",
            "custom_details": {
                "channel_id": channel.id,
                "channel_name": channel.name,
                "test": true,
                "alertmanager": message.payload,
            }
        }
    })
}

fn decrypt_channel_secret(
    state: &AppState,
    channel: &NotificationChannelRecord,
) -> Result<Value, AppError> {
    let encrypted_secret = channel.secret_encrypted.as_deref().ok_or_else(|| {
        AppError::validation_failed("notification channel secret is not configured")
    })?;

    decrypt_secret_text(state, encrypted_secret)
}

fn encrypt_secret_to_text(state: &AppState, secret: &Value) -> Result<String, AppError> {
    let plaintext = serde_json::to_vec(secret)
        .map_err(|error| AppError::crypto(format!("notification secret invalid: {error}")))?;
    let envelope = encrypt_bytes(&state.config.security.master_key, &plaintext)?;

    serde_json::to_string(&envelope).map_err(|error| {
        AppError::crypto(format!(
            "notification secret envelope serialization failed: {error}"
        ))
    })
}

fn decrypt_secret_text(state: &AppState, encrypted_secret: &str) -> Result<Value, AppError> {
    let envelope: PrivateKeyEnvelope = serde_json::from_str(encrypted_secret).map_err(|error| {
        AppError::crypto(format!("notification secret envelope invalid: {error}"))
    })?;
    let plaintext = decrypt_bytes(&state.config.security.master_key, &envelope)?;

    serde_json::from_slice(&plaintext).map_err(|error| {
        AppError::crypto(format!("notification secret plaintext invalid: {error}"))
    })
}

fn apply_public_summary(
    kind: &str,
    config: &mut Value,
    secret: Option<&Value>,
) -> Result<(), AppError> {
    let summary = build_target_summary(kind, config, secret)?;
    let Value::Object(map) = config else {
        return Err(AppError::validation_failed(
            "notification channel config must be an object",
        ));
    };

    if let Some(summary) = summary {
        map.insert("target_summary".to_owned(), Value::String(summary));
    }

    Ok(())
}

fn build_target_summary(
    kind: &str,
    config: &Value,
    secret: Option<&Value>,
) -> Result<Option<String>, AppError> {
    match kind {
        "webhook" => {
            let url = secret.and_then(|secret| {
                text_field(secret, "url").or_else(|| text_field(secret, "webhook_url"))
            });
            url.map(|url| public_url_summary(url, "webhook url"))
                .transpose()
        }
        "email" => {
            let host = optional_text_config(config, "smtp_host")?.unwrap_or("smtp");
            let to = target_recipient_summary(config);

            Ok(Some(format!("{host} -> {to}")))
        }
        "pagerduty" => Ok(optional_text_config(config, "service")?
            .map(|service| format!("PagerDuty: {service}"))
            .or_else(|| Some("PagerDuty".to_owned()))),
        _ => Ok(None),
    }
}

fn public_url_summary(url: &str, label: &str) -> Result<String, AppError> {
    validate_http_url(url, label)?;
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AppError::validation_failed(format!("{label} is invalid")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::validation_failed(format!("{label} host is required")))?;
    let port = parsed
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();

    Ok(format!("{}://{}{}", parsed.scheme(), host, port))
}

fn validate_http_url(url: &str, label: &str) -> Result<(), AppError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|_| AppError::validation_failed(format!("{label} is invalid")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::validation_failed(format!(
            "{label} scheme must be http or https"
        )));
    }
    if parsed.host_str().is_none() {
        return Err(AppError::validation_failed(format!(
            "{label} host is required"
        )));
    }

    Ok(())
}

fn require_text_config<'a>(config: &'a Value, key: &str) -> Result<&'a str, AppError> {
    text_field(config, key).ok_or_else(|| {
        AppError::validation_failed(format!("notification config {key} is required"))
    })
}

fn require_number_config(config: &Value, key: &str) -> Result<u64, AppError> {
    let value = config.get(key).and_then(Value::as_u64).ok_or_else(|| {
        AppError::validation_failed(format!("notification config {key} is required"))
    })?;
    if value == 0 || value > u64::from(u16::MAX) {
        return Err(AppError::validation_failed(format!(
            "notification config {key} is invalid"
        )));
    }

    Ok(value)
}

fn require_text_or_array_config(config: &Value, key: &str) -> Result<(), AppError> {
    if text_field(config, key).is_some() {
        return Ok(());
    }
    let valid_array = config
        .get(key)
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty() && items.iter().all(|item| item.as_str().is_some()));
    if valid_array {
        return Ok(());
    }

    Err(AppError::validation_failed(format!(
        "notification config {key} is required"
    )))
}

fn optional_text_config<'a>(config: &'a Value, key: &str) -> Result<Option<&'a str>, AppError> {
    match config.get(key) {
        Some(Value::String(value)) if !value.trim().is_empty() => Ok(Some(value.trim())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(AppError::validation_failed(format!(
            "notification config {key} must be text"
        ))),
    }
}

fn require_text_secret<'a>(secret: &'a Value, key: &str) -> Result<&'a str, AppError> {
    text_field(secret, key).ok_or_else(|| {
        AppError::validation_failed(format!("notification secret {key} is required"))
    })
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty() && !text.contains('\0')).then_some(text)
}

fn number_field(value: &Value, key: &str) -> Option<u16> {
    let value = value.get(key)?.as_u64()?;
    u16::try_from(value).ok().filter(|value| *value > 0)
}

fn recipients_field(value: &Value, key: &str) -> Result<Vec<String>, String> {
    if let Some(text) = text_field(value, key) {
        return Ok(vec![text.to_owned()]);
    }
    let Some(items) = value.get(key).and_then(Value::as_array) else {
        return Err(format!("{key} is not configured"));
    };
    let recipients = items
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if recipients.is_empty() {
        return Err(format!("{key} is not configured"));
    }

    Ok(recipients)
}

fn ensure_success_status(status: reqwest::StatusCode, target: &str) -> Result<(), String> {
    if status.is_success() {
        return Ok(());
    }

    Err(format!("{target} returned HTTP {status}"))
}

fn target_recipient_summary(config: &Value) -> String {
    if let Some(to) = text_field(config, "to") {
        return to.to_owned();
    }

    let Some(items) = config.get("to").and_then(Value::as_array) else {
        return "recipients".to_owned();
    };
    match items.len() {
        0 => "recipients".to_owned(),
        1 => items[0]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| "recipients".to_owned()),
        len => format!("{len} recipients"),
    }
}

impl From<NotificationChannelRecord> for NotificationChannel {
    fn from(record: NotificationChannelRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            kind: record.kind,
            enabled: record.enabled,
            config: record.config_json,
            secret_configured: record.secret_encrypted.is_some(),
            last_test_status: record.last_test_status,
            last_test_error: record.last_test_error,
            last_test_at: record.last_test_at,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

fn notification_channel_audit_json(record: &NotificationChannelRecord) -> Value {
    json!({
        "id": record.id,
        "name": &record.name,
        "kind": &record.kind,
        "enabled": record.enabled,
        "config": &record.config_json,
        "secret_configured": record.secret_encrypted.is_some(),
        "last_test_status": &record.last_test_status,
        "last_test_error": &record.last_test_error,
        "last_test_at": &record.last_test_at,
        "created_at": &record.created_at,
        "updated_at": &record.updated_at,
    })
}

fn truncate_test_error(error: &str) -> String {
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
    AppError::dependency(format!("notification channel database error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use uuid::Uuid;

    use crate::modules::auth::session::AdminContext;

    use super::{
        build_test_delivery_message, deliver_webhook_test, ensure_admin_permission,
        normalize_channel_name, normalize_config, normalize_kind, normalize_test_mode,
        public_url_summary, validate_email_channel, validate_webhook_secret,
        NotificationChannelRecord, NotificationChannelTestMode, TestNotificationChannelRequest,
    };

    #[test]
    fn kind_and_name_are_validated() {
        assert_eq!(normalize_kind(" WebHook ").expect("kind"), "webhook");
        assert!(normalize_kind("sms").is_err());
        assert_eq!(
            normalize_channel_name(" Ops Alerts ").expect("name"),
            "Ops Alerts"
        );
        assert!(normalize_channel_name("").is_err());
    }

    #[test]
    fn public_config_rejects_sensitive_keys() {
        assert!(normalize_config(json!({"smtp_host": "smtp.example.com"})).is_ok());
        assert!(normalize_config(json!({"smtp_password": "secret"})).is_err());
        assert!(normalize_config(json!({"headers": {"token": "secret"}})).is_err());
    }

    #[test]
    fn webhook_secret_must_contain_http_url() {
        assert!(validate_webhook_secret(&json!({"url": "https://hooks.example.com/a"})).is_ok());
        assert!(validate_webhook_secret(&json!({"url": "ftp://example.com/a"})).is_err());
        assert_eq!(
            public_url_summary("https://hooks.example.com/a/token", "webhook url")
                .expect("summary"),
            "https://hooks.example.com"
        );
    }

    #[test]
    fn delivery_test_mode_requires_explicit_confirmation() {
        assert_eq!(
            normalize_test_mode(&TestNotificationChannelRequest::default()).expect("mode"),
            NotificationChannelTestMode::DryRun
        );
        assert!(normalize_test_mode(&TestNotificationChannelRequest {
            mode: Some("delivery".to_owned()),
            confirm_delivery: false,
        })
        .is_err());
        assert_eq!(
            normalize_test_mode(&TestNotificationChannelRequest {
                mode: Some("delivery".to_owned()),
                confirm_delivery: true,
            })
            .expect("mode"),
            NotificationChannelTestMode::Delivery
        );
    }

    #[tokio::test]
    async fn webhook_delivery_test_posts_payload_to_configured_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener");
        let address = listener.local_addr().expect("local address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = vec![0_u8; 8192];
            let read = stream.read(&mut buffer).await.expect("read");
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .expect("write response");

            request
        });

        let channel = NotificationChannelRecord {
            id: Uuid::nil(),
            name: "Ops Webhook".to_owned(),
            kind: "webhook".to_owned(),
            enabled: true,
            config_json: json!({}),
            secret_encrypted: None,
            last_test_status: None,
            last_test_error: None,
            last_test_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let message = build_test_delivery_message(&channel);
        let client = reqwest::Client::new();
        deliver_webhook_test(
            &client,
            &channel,
            &json!({ "url": format!("http://{address}/hook") }),
            &message,
        )
        .await
        .expect("webhook delivery");

        let request = server.await.expect("server task");
        assert!(request.starts_with("POST /hook "));
        assert!(request.contains("\"source\":\"admin-test\""));
        assert!(request.contains("\"channel_name\":\"Ops Webhook\""));
        assert!(request.contains("\"alertname\":\"NotificationChannelTest\""));
    }

    #[test]
    fn email_channel_requires_public_and_secret_smtp_fields() {
        let config = json!({
            "smtp_host": "smtp.example.com",
            "smtp_port": 587,
            "smtp_user": "alerts@example.com",
            "from": "alerts@example.com",
            "to": ["ops@example.com"]
        });
        let secret = json!({"smtp_password": "secret"});
        assert!(validate_email_channel(&config, &secret).is_ok());
        assert!(validate_email_channel(&config, &json!({})).is_err());
    }

    #[test]
    fn permission_check_uses_notification_permissions() {
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
            permissions: vec!["notification:read".to_owned()],
        };

        assert!(ensure_admin_permission(&admin, "notification:read").is_ok());
        assert!(ensure_admin_permission(&admin, "notification:update").is_err());
        admin.permissions.push("notification:update".to_owned());
        assert!(ensure_admin_permission(&admin, "notification:update").is_ok());
    }
}
