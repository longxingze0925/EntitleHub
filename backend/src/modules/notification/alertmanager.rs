use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap},
    Json,
};
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::FromRow;
use std::time::Instant;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, PrivateKeyEnvelope},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    metrics::{record_notification_delivery, NotificationDeliveryStatus},
    state::AppState,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertmanagerWebhookPayload {
    pub status: String,
    pub receiver: Option<String>,
    pub group_key: Option<String>,
    #[serde(default)]
    pub common_labels: Value,
    #[serde(default)]
    pub common_annotations: Value,
    pub external_url: Option<String>,
    #[serde(default)]
    pub alerts: Vec<AlertmanagerAlert>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertmanagerAlert {
    pub status: String,
    #[serde(default)]
    pub labels: Value,
    #[serde(default)]
    pub annotations: Value,
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    pub generator_url: Option<String>,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AlertDeliveryResponse {
    pub accepted: bool,
    pub channel_count: usize,
    pub delivered: usize,
    pub failed: usize,
    pub failures: Vec<ChannelDeliveryFailure>,
}

#[derive(Debug, Serialize)]
pub struct ChannelDeliveryFailure {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub kind: String,
    pub error: String,
}

#[derive(Debug, FromRow)]
struct NotificationChannelRecord {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    kind: String,
    config_json: Value,
    secret_encrypted: Option<String>,
}

struct DeliveryMessage {
    summary: String,
    severity: String,
    source: String,
    body: String,
    webhook_payload: Value,
}

pub async fn receive_alertmanager_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::Extension(request_id): axum::Extension<RequestId>,
    Json(payload): Json<AlertmanagerWebhookPayload>,
) -> Result<Json<ApiResponse<AlertDeliveryResponse>>, AppError> {
    ensure_alertmanager_token(state.config.alerting.webhook_token.as_deref(), &headers)?;
    validate_alertmanager_payload(&payload)?;

    let channels = list_enabled_channels(&state).await?;
    let message = build_delivery_message(&payload);
    let client = reqwest::Client::builder()
        .timeout(state.config.alerting.delivery_timeout)
        .build()
        .map_err(|error| AppError::dependency(format!("alert delivery client failed: {error}")))?;

    let mut delivered = 0_usize;
    let mut failures = Vec::new();
    for channel in &channels {
        let started_at = Instant::now();
        let result = deliver_channel(&state, &client, channel, &message).await;
        let delivery_status = if result.is_ok() {
            NotificationDeliveryStatus::Success
        } else {
            NotificationDeliveryStatus::Failure
        };
        record_notification_delivery(&channel.kind, delivery_status, started_at.elapsed());

        match result {
            Ok(()) => delivered += 1,
            Err(error) => {
                tracing::warn!(
                    channel_id = %channel.id,
                    tenant_id = %channel.tenant_id,
                    kind = %channel.kind,
                    %error,
                    "notification alert delivery failed"
                );
                failures.push(ChannelDeliveryFailure {
                    channel_id: channel.id,
                    channel_name: channel.name.clone(),
                    kind: channel.kind.clone(),
                    error,
                });
            }
        }
    }

    if !channels.is_empty() && delivered == 0 && !failures.is_empty() {
        return Err(AppError::dependency(
            "all notification channel deliveries failed",
        ));
    }

    let response = AlertDeliveryResponse {
        accepted: true,
        channel_count: channels.len(),
        delivered,
        failed: failures.len(),
        failures,
    };

    Ok(Json(ApiResponse::ok(response, request_id.to_string())))
}

async fn list_enabled_channels(
    state: &AppState,
) -> Result<Vec<NotificationChannelRecord>, AppError> {
    sqlx::query_as::<_, NotificationChannelRecord>(
        r#"
        select
          id,
          tenant_id,
          name,
          kind,
          config_json,
          secret_encrypted
        from notification_channels
        where enabled = true
          and secret_encrypted is not null
        order by created_at asc, id asc
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn deliver_channel(
    state: &AppState,
    client: &reqwest::Client,
    channel: &NotificationChannelRecord,
    message: &DeliveryMessage,
) -> Result<(), String> {
    let secret = decrypt_channel_secret(state, channel).map_err(|error| error.to_string())?;
    match channel.kind.as_str() {
        "webhook" => deliver_webhook(client, channel, &secret, message).await,
        "email" => deliver_email(channel, &secret, message).await,
        "pagerduty" => deliver_pagerduty(client, channel, &secret, message).await,
        _ => Err("notification channel kind is invalid".to_owned()),
    }
}

async fn deliver_webhook(
    client: &reqwest::Client,
    channel: &NotificationChannelRecord,
    secret: &Value,
    message: &DeliveryMessage,
) -> Result<(), String> {
    let url = text_secret(secret, "url")
        .or_else(|| text_secret(secret, "webhook_url"))
        .ok_or_else(|| "webhook url is not configured".to_owned())?;
    validate_http_url(url, "webhook url")?;

    let response = client
        .post(url)
        .header("user-agent", "user-admin-alertmanager-adapter/1")
        .json(&json!({
            "schema_version": 1,
            "source": "alertmanager",
            "channel_id": channel.id,
            "channel_name": channel.name,
            "summary": message.summary,
            "severity": message.severity,
            "alert_source": message.source,
            "payload": message.webhook_payload,
        }))
        .send()
        .await
        .map_err(|error| format!("webhook send failed: {error}"))?;
    ensure_success_status(response.status(), "webhook")
}

async fn deliver_pagerduty(
    client: &reqwest::Client,
    channel: &NotificationChannelRecord,
    secret: &Value,
    message: &DeliveryMessage,
) -> Result<(), String> {
    let routing_key = text_secret(secret, "routing_key")
        .ok_or_else(|| "pagerduty routing_key is not configured".to_owned())?;
    let payload = build_pagerduty_event(channel, routing_key, message);
    let response = client
        .post("https://events.pagerduty.com/v2/enqueue")
        .header("user-agent", "user-admin-alertmanager-adapter/1")
        .json(&payload)
        .send()
        .await
        .map_err(|error| format!("pagerduty send failed: {error}"))?;
    ensure_success_status(response.status(), "pagerduty")
}

async fn deliver_email(
    channel: &NotificationChannelRecord,
    secret: &Value,
    message: &DeliveryMessage,
) -> Result<(), String> {
    let host = text_config(&channel.config_json, "smtp_host")
        .ok_or_else(|| "smtp_host is not configured".to_owned())?;
    let port = number_config(&channel.config_json, "smtp_port").unwrap_or(587);
    let from = text_config(&channel.config_json, "from")
        .ok_or_else(|| "email from is not configured".to_owned())?;
    let recipients = recipients_config(&channel.config_json, "to")?;
    let user =
        text_config(&channel.config_json, "smtp_user").or_else(|| text_secret(secret, "smtp_user"));
    let password = text_secret(secret, "smtp_password")
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

fn build_delivery_message(payload: &AlertmanagerWebhookPayload) -> DeliveryMessage {
    let summary = alert_summary(payload);
    let severity = label_text(&payload.common_labels, "severity")
        .unwrap_or("warning")
        .to_ascii_lowercase();
    let source = payload
        .external_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("alertmanager")
        .to_owned();
    let body = alert_text_body(payload, &summary, &severity, &source);
    let webhook_payload = serde_json::to_value(payload).unwrap_or_else(|_| json!({}));

    DeliveryMessage {
        summary,
        severity,
        source,
        body,
        webhook_payload,
    }
}

fn alert_summary(payload: &AlertmanagerWebhookPayload) -> String {
    annotation_text(&payload.common_annotations, "summary")
        .or_else(|| annotation_text(&payload.common_annotations, "description"))
        .or_else(|| label_text(&payload.common_labels, "alertname"))
        .map(str::to_owned)
        .unwrap_or_else(|| {
            format!(
                "{} 条告警 {}",
                payload.alerts.len(),
                alert_status_text(&payload.status)
            )
        })
}

fn alert_text_body(
    payload: &AlertmanagerWebhookPayload,
    summary: &str,
    severity: &str,
    source: &str,
) -> String {
    let mut lines = vec![
        format!("状态：{}", alert_status_text(&payload.status)),
        format!("级别：{severity}"),
        format!("摘要：{summary}"),
        format!("来源：{source}"),
    ];
    if let Some(group_key) = payload.group_key.as_deref() {
        lines.push(format!("分组：{group_key}"));
    }
    if let Some(receiver) = payload.receiver.as_deref() {
        lines.push(format!("接收器：{receiver}"));
    }
    lines.push(String::new());
    lines.push(format!("告警数量：{}", payload.alerts.len()));

    for alert in &payload.alerts {
        let alert_name = label_text(&alert.labels, "alertname").unwrap_or("alert");
        let instance = label_text(&alert.labels, "instance").unwrap_or("-");
        let description = annotation_text(&alert.annotations, "description")
            .or_else(|| annotation_text(&alert.annotations, "summary"))
            .unwrap_or("");
        lines.push(format!(
            "- [{}] {} 实例={} {}",
            alert_status_text(&alert.status),
            alert_name,
            instance,
            description
        ));
        if let Some(generator_url) = alert.generator_url.as_deref() {
            if !generator_url.trim().is_empty() {
                lines.push(format!("  {generator_url}"));
            }
        }
    }

    lines.join("\n")
}

fn alert_status_text(status: &str) -> &'static str {
    match status {
        "firing" => "触发中",
        "resolved" => "已恢复",
        _ => "未知",
    }
}

fn build_pagerduty_event(
    channel: &NotificationChannelRecord,
    routing_key: &str,
    message: &DeliveryMessage,
) -> Value {
    let action = if message
        .webhook_payload
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status == "resolved")
    {
        "resolve"
    } else {
        "trigger"
    };
    let dedup_key = message
        .webhook_payload
        .get("groupKey")
        .and_then(Value::as_str)
        .unwrap_or(&message.summary);

    json!({
        "routing_key": routing_key,
        "event_action": action,
        "dedup_key": dedup_key,
        "payload": {
            "summary": message.summary,
            "source": message.source,
            "severity": pagerduty_severity(&message.severity),
            "custom_details": {
                "channel_id": channel.id,
                "channel_name": channel.name,
                "alertmanager": message.webhook_payload,
            }
        }
    })
}

fn pagerduty_severity(severity: &str) -> &'static str {
    match severity {
        "critical" => "critical",
        "warning" => "warning",
        "info" | "information" => "info",
        _ => "error",
    }
}

fn validate_alertmanager_payload(payload: &AlertmanagerWebhookPayload) -> Result<(), AppError> {
    let status = payload.status.trim();
    if !matches!(status, "firing" | "resolved") {
        return Err(AppError::validation_failed(
            "alertmanager status must be firing or resolved",
        ));
    }

    Ok(())
}

fn ensure_alertmanager_token(expected: Option<&str>, headers: &HeaderMap) -> Result<(), AppError> {
    let expected = expected
        .ok_or_else(|| AppError::forbidden("alertmanager webhook token is not configured"))?;
    let provided = bearer_token(headers).or_else(|| header_text(headers, "x-alertmanager-token"));
    let provided =
        provided.ok_or_else(|| AppError::forbidden("alertmanager webhook token is required"))?;

    if provided.as_bytes().ct_eq(expected.as_bytes()).into() {
        return Ok(());
    }

    Err(AppError::forbidden("alertmanager webhook token is invalid"))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?.trim();
    value
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn decrypt_channel_secret(
    state: &AppState,
    channel: &NotificationChannelRecord,
) -> Result<Value, AppError> {
    let encrypted_secret = channel.secret_encrypted.as_deref().ok_or_else(|| {
        AppError::validation_failed("notification channel secret is not configured")
    })?;
    let envelope: PrivateKeyEnvelope = serde_json::from_str(encrypted_secret).map_err(|error| {
        AppError::crypto(format!("notification secret envelope invalid: {error}"))
    })?;
    let plaintext = decrypt_bytes(&state.config.security.master_key, &envelope)?;

    serde_json::from_slice(&plaintext).map_err(|error| {
        AppError::crypto(format!("notification secret plaintext invalid: {error}"))
    })
}

fn validate_http_url(url: &str, label: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("{label} is invalid"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("{label} scheme must be http or https"));
    }
    if parsed.host_str().is_none() {
        return Err(format!("{label} host is required"));
    }

    Ok(())
}

fn ensure_success_status(status: StatusCode, target: &str) -> Result<(), String> {
    if status.is_success() {
        return Ok(());
    }

    Err(format!("{target} returned HTTP {status}"))
}

fn text_config<'a>(config: &'a Value, key: &str) -> Option<&'a str> {
    text_field(config, key)
}

fn text_secret<'a>(secret: &'a Value, key: &str) -> Option<&'a str> {
    text_field(secret, key)
}

fn text_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty() && !text.contains('\0')).then_some(text)
}

fn number_config(config: &Value, key: &str) -> Option<u16> {
    let value = config.get(key)?.as_u64()?;
    u16::try_from(value).ok().filter(|value| *value > 0)
}

fn recipients_config(config: &Value, key: &str) -> Result<Vec<String>, String> {
    if let Some(text) = text_config(config, key) {
        return Ok(vec![text.to_owned()]);
    }
    let Some(items) = config.get(key).and_then(Value::as_array) else {
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

fn label_text<'a>(labels: &'a Value, key: &str) -> Option<&'a str> {
    text_field(labels, key)
}

fn annotation_text<'a>(annotations: &'a Value, key: &str) -> Option<&'a str> {
    text_field(annotations, key)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("notification channel database error: {error}"))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    use super::{
        alert_summary, build_delivery_message, build_pagerduty_event, ensure_alertmanager_token,
        validate_alertmanager_payload, AlertmanagerAlert, AlertmanagerWebhookPayload,
        NotificationChannelRecord,
    };

    #[test]
    fn alertmanager_token_accepts_bearer_or_custom_header() {
        let expected = Some("alertmanager-webhook-token-32-bytes");

        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer alertmanager-webhook-token-32-bytes"),
        );
        assert!(ensure_alertmanager_token(expected, &headers).is_ok());

        headers.clear();
        headers.insert(
            "x-alertmanager-token",
            HeaderValue::from_static("alertmanager-webhook-token-32-bytes"),
        );
        assert!(ensure_alertmanager_token(expected, &headers).is_ok());

        headers.insert("x-alertmanager-token", HeaderValue::from_static("wrong"));
        assert!(ensure_alertmanager_token(expected, &headers).is_err());
        assert!(ensure_alertmanager_token(None, &headers).is_err());
    }

    #[test]
    fn alertmanager_payload_builds_summary_and_body() {
        let payload = sample_payload();
        validate_alertmanager_payload(&payload).expect("payload");
        let message = build_delivery_message(&payload);

        assert_eq!(alert_summary(&payload), "High error rate");
        assert_eq!(message.severity, "critical");
        assert!(message.body.contains("告警数量：1"));
        assert!(message.body.contains("api-1"));
    }

    #[test]
    fn pagerduty_event_uses_group_key_as_dedup_key() {
        let payload = sample_payload();
        let message = build_delivery_message(&payload);
        let channel = NotificationChannelRecord {
            id: uuid::Uuid::nil(),
            tenant_id: uuid::Uuid::nil(),
            name: "pd".to_owned(),
            kind: "pagerduty".to_owned(),
            config_json: json!({}),
            secret_encrypted: None,
        };
        let event = build_pagerduty_event(&channel, "route-key", &message);

        assert_eq!(event["routing_key"], "route-key");
        assert_eq!(event["event_action"], "trigger");
        assert_eq!(event["dedup_key"], "{}:{alertname=\"HighErrorRate\"}");
        assert_eq!(event["payload"]["severity"], "critical");
    }

    fn sample_payload() -> AlertmanagerWebhookPayload {
        AlertmanagerWebhookPayload {
            status: "firing".to_owned(),
            receiver: Some("backend".to_owned()),
            group_key: Some("{}:{alertname=\"HighErrorRate\"}".to_owned()),
            common_labels: json!({
                "alertname": "HighErrorRate",
                "severity": "critical",
            }),
            common_annotations: json!({
                "summary": "High error rate",
            }),
            external_url: Some("http://prometheus:9090".to_owned()),
            alerts: vec![AlertmanagerAlert {
                status: "firing".to_owned(),
                labels: json!({
                    "alertname": "HighErrorRate",
                    "instance": "api-1",
                }),
                annotations: json!({
                    "description": "5xx rate is above threshold",
                }),
                starts_at: Some("2026-06-06T08:00:00Z".to_owned()),
                ends_at: None,
                generator_url: Some("http://prometheus/graph".to_owned()),
                fingerprint: Some("abc123".to_owned()),
            }],
        }
    }
}
