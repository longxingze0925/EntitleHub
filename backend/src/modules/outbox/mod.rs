use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub mod admin;
pub mod worker;

use crate::{
    crypto::envelope::{encrypt_bytes, PrivateKeyEnvelope},
    error::AppError,
    state::AppState,
};

const BODY_FORMAT_TEXT: &str = "text/plain";

pub async fn enqueue_admin_password_reset_email(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    tenant_id: Uuid,
    to: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    enqueue_token_email(
        transaction,
        state,
        Some(tenant_id),
        "email.admin_password_reset",
        to,
        "重置管理员密码",
        "请使用以下令牌重置管理员密码。",
        "admin/password-reset",
        token,
        expires_at,
    )
    .await
}

pub async fn enqueue_team_member_email_verify_email(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    tenant_id: Uuid,
    to: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    enqueue_token_email(
        transaction,
        state,
        Some(tenant_id),
        "email.team_member_email_verify",
        to,
        "验证管理员邮箱",
        "请使用以下令牌验证管理员邮箱地址。",
        "admin/email-verify",
        token,
        expires_at,
    )
    .await
}

pub async fn enqueue_team_invite_email(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    tenant_id: Uuid,
    to: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    enqueue_token_email(
        transaction,
        state,
        Some(tenant_id),
        "email.team_invite",
        to,
        "接受团队邀请",
        "请使用以下令牌接受团队邀请。",
        "team/invitations/accept",
        token,
        expires_at,
    )
    .await
}

pub async fn enqueue_customer_password_reset_email(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    tenant_id: Uuid,
    to: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    enqueue_token_email(
        transaction,
        state,
        Some(tenant_id),
        "email.customer_password_reset",
        to,
        "重置密码",
        "请使用以下令牌重置密码。",
        "client/password-reset",
        token,
        expires_at,
    )
    .await
}

pub async fn enqueue_customer_email_verify_email(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    tenant_id: Uuid,
    to: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    enqueue_token_email(
        transaction,
        state,
        Some(tenant_id),
        "email.customer_email_verify",
        to,
        "验证邮箱",
        "请使用以下令牌验证邮箱地址。",
        "client/email-verify",
        token,
        expires_at,
    )
    .await
}

async fn enqueue_token_email(
    transaction: &mut Transaction<'_, Postgres>,
    state: &AppState,
    tenant_id: Option<Uuid>,
    event_type: &str,
    to: &str,
    subject: &str,
    intro: &str,
    action_path: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    let body = token_email_body(
        intro,
        state.config.app.base_url.as_deref(),
        action_path,
        token,
        expires_at,
    );
    let payload =
        build_encrypted_email_payload(&state.config.security.master_key, to, subject, &body)?;

    enqueue_event_in_transaction(transaction, tenant_id, event_type, payload).await
}

async fn enqueue_event_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Option<Uuid>,
    event_type: &str,
    payload: Value,
) -> Result<Uuid, AppError> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
        insert into outbox_events (
          id,
          tenant_id,
          event_type,
          payload
        )
        values ($1, $2, $3, $4)
        "#,
    )
    .bind(id)
    .bind(tenant_id)
    .bind(event_type)
    .bind(payload)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(id)
}

fn build_encrypted_email_payload(
    master_key: &[u8; 32],
    to: &str,
    subject: &str,
    text_body: &str,
) -> Result<Value, AppError> {
    let body_envelope = encrypt_bytes(master_key, text_body.as_bytes())?;
    let body_envelope = envelope_to_value(body_envelope)?;

    Ok(json!({
        "schema_version": 1,
        "kind": "email",
        "to": to,
        "subject": subject,
        "body_format": BODY_FORMAT_TEXT,
        "body_envelope": body_envelope,
    }))
}

fn token_email_body(
    intro: &str,
    base_url: Option<&str>,
    action_path: &str,
    token: &str,
    expires_at: DateTime<Utc>,
) -> String {
    let mut lines = vec![
        intro.to_owned(),
        String::new(),
        format!("令牌：{token}"),
        format!("过期时间：{}", expires_at.to_rfc3339()),
    ];

    if let Some(url) = token_action_url(base_url, action_path, token) {
        lines.push(String::new());
        lines.push(format!("链接：{url}"));
    }

    lines.push(String::new());
    lines.push("如果这不是你本人操作，请忽略此邮件。".to_owned());
    lines.join("\n")
}

fn token_action_url(base_url: Option<&str>, action_path: &str, token: &str) -> Option<String> {
    let base_url = base_url?.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return None;
    }
    let action_path = action_path.trim_start_matches('/');

    Some(format!("{base_url}/{action_path}?token={token}"))
}

fn envelope_to_value(envelope: PrivateKeyEnvelope) -> Result<Value, AppError> {
    serde_json::to_value(envelope)
        .map_err(|error| AppError::crypto(format!("email envelope serialization failed: {error}")))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("outbox database error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use crate::crypto::envelope::{decrypt_bytes, PrivateKeyEnvelope};

    use super::{build_encrypted_email_payload, token_action_url, token_email_body};

    #[test]
    fn encrypted_email_payload_does_not_include_plaintext_body() {
        let payload = build_encrypted_email_payload(
            &[9_u8; 32],
            "user@example.com",
            "Secret subject",
            "secret-reset-token",
        )
        .expect("payload should build");
        let serialized = serde_json::to_string(&payload).expect("payload should serialize");

        assert!(serialized.contains("user@example.com"));
        assert!(serialized.contains("Secret subject"));
        assert!(!serialized.contains("secret-reset-token"));

        let envelope: PrivateKeyEnvelope =
            serde_json::from_value(payload["body_envelope"].clone()).expect("body envelope");
        let body = decrypt_bytes(&[9_u8; 32], &envelope).expect("body should decrypt");

        assert_eq!(body, b"secret-reset-token");
    }

    #[test]
    fn token_body_contains_link_only_when_base_url_is_configured() {
        let expires_at = Utc.with_ymd_and_hms(2026, 6, 4, 8, 30, 0).unwrap();

        let without_link = token_email_body("Intro.", None, "reset", "token-value", expires_at);
        let with_link = token_email_body(
            "Intro.",
            Some("https://admin.example.com/"),
            "/reset",
            "token-value",
            expires_at,
        );

        assert!(without_link.contains("令牌：token-value"));
        assert!(!without_link.contains("链接："));
        assert!(with_link.contains("链接：https://admin.example.com/reset?token=token-value"));
    }

    #[test]
    fn token_action_url_trims_slashes_and_blank_base_url() {
        assert_eq!(
            token_action_url(Some("https://example.com/"), "/verify", "token"),
            Some("https://example.com/verify?token=token".to_owned())
        );
        assert_eq!(token_action_url(Some(" "), "/verify", "token"), None);
        assert_eq!(token_action_url(None, "/verify", "token"), None);
    }
}
