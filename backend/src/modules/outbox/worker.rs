use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, PrivateKeyEnvelope},
    metrics,
    modules::system::email::{
        load_email_delivery_config, send_email, EmailDeliveryConfig, EmailDeliveryMessage,
    },
    state::AppState,
};

#[derive(Debug, FromRow)]
struct EmailOutboxJob {
    id: Uuid,
    payload: Value,
    attempts: i32,
}

pub fn spawn_email_outbox_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        run_email_outbox_worker(state).await;
    })
}

async fn run_email_outbox_worker(state: AppState) {
    let mut interval = tokio::time::interval(state.config.email.outbox_poll_interval);
    loop {
        interval.tick().await;
        if let Err(error) = process_pending_email_batch(&state).await {
            tracing::warn!(%error, "email outbox worker tick failed");
        }
    }
}

async fn process_pending_email_batch(state: &AppState) -> Result<(), String> {
    let Some(email_config) = load_email_delivery_config(state).await? else {
        return Ok(());
    };
    let jobs = claim_pending_email_jobs(state)
        .await
        .map_err(|error| format!("claim email outbox jobs failed: {error}"))?;
    if jobs.is_empty() {
        return Ok(());
    }

    for job in jobs {
        let job_id = job.id;
        let attempts = job.attempts;
        if let Err(error) = process_email_job(state, &email_config, job).await {
            if let Err(mark_error) = mark_job_failed(state, job_id, attempts, &error).await {
                tracing::warn!(%mark_error, %job_id, "mark email outbox job failed state failed");
            }
            tracing::warn!(%error, "email outbox job failed");
        }
    }

    Ok(())
}

async fn process_email_job(
    state: &AppState,
    email_config: &EmailDeliveryConfig,
    job: EmailOutboxJob,
) -> Result<(), String> {
    let message = parse_email_payload(&job.payload, &state.config.security.master_key)?;
    send_email(email_config, &message).await?;
    mark_job_processed(state, job.id)
        .await
        .map_err(|error| format!("mark email outbox job processed failed: {error}"))
}

async fn claim_pending_email_jobs(state: &AppState) -> Result<Vec<EmailOutboxJob>, sqlx::Error> {
    sqlx::query_as::<_, EmailOutboxJob>(
        r#"
        update outbox_events
        set
          status = 'processing',
          attempts = attempts + 1,
          last_error = null,
          next_run_at = now() + ($2::bigint * interval '1 second')
        where id in (
          select id
          from outbox_events
          where event_type like 'email.%'
            and next_run_at <= now()
            and status in ('pending', 'processing')
          order by next_run_at asc, created_at asc
          limit $1
          for update skip locked
        )
        returning
          id,
          payload,
          attempts
        "#,
    )
    .bind(state.config.email.outbox_batch_size)
    .bind(state.config.email.outbox_processing_timeout.as_secs() as i64)
    .fetch_all(&state.db)
    .await
}

async fn mark_job_processed(state: &AppState, job_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        update outbox_events
        set
          status = 'processed',
          processed_at = now(),
          last_error = null
        where id = $1
        "#,
    )
    .bind(job_id)
    .execute(&state.db)
    .await
    .map(|_| ())
}

async fn mark_job_failed(
    state: &AppState,
    job_id: Uuid,
    attempts: i32,
    error: &str,
) -> Result<(), sqlx::Error> {
    let error = truncate_error(error);
    if attempts >= state.config.email.outbox_max_attempts {
        sqlx::query(
            r#"
            update outbox_events
            set
              status = 'failed',
              last_error = $2,
              processed_at = now()
            where id = $1
            "#,
        )
        .bind(job_id)
        .bind(error)
        .execute(&state.db)
        .await
        .map(|_| {
            metrics::record_worker_job_failed();
        })
    } else {
        sqlx::query(
            r#"
            update outbox_events
            set
              status = 'pending',
              last_error = $2,
              next_run_at = now() + ($3::bigint * interval '1 second')
            where id = $1
            "#,
        )
        .bind(job_id)
        .bind(error)
        .bind(retry_delay_seconds(attempts))
        .execute(&state.db)
        .await
        .map(|_| ())
    }
}

fn parse_email_payload(
    payload: &Value,
    master_key: &[u8; 32],
) -> Result<EmailDeliveryMessage, String> {
    let to = required_string(payload, "to")?.to_owned();
    let subject = required_string(payload, "subject")?.to_owned();
    let body_envelope = payload
        .get("body_envelope")
        .ok_or_else(|| "email payload body_envelope is missing".to_owned())?;
    let body_envelope: PrivateKeyEnvelope = serde_json::from_value(body_envelope.clone())
        .map_err(|error| format!("email payload body_envelope is invalid: {error}"))?;
    let body = decrypt_bytes(master_key, &body_envelope)
        .map_err(|error| format!("email body decrypt failed: {error}"))?;
    let body =
        String::from_utf8(body).map_err(|error| format!("email body is not utf8: {error}"))?;

    Ok(EmailDeliveryMessage { to, subject, body })
}

fn required_string<'a>(payload: &'a Value, field: &str) -> Result<&'a str, String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("email payload {field} is missing"))
}

fn retry_delay_seconds(attempts: i32) -> i64 {
    let exponent = attempts.clamp(1, 6) - 1;
    30 * 2_i64.pow(exponent as u32)
}

fn truncate_error(error: &str) -> String {
    const MAX_ERROR_LEN: usize = 2_000;
    if error.len() <= MAX_ERROR_LEN {
        return error.to_owned();
    }

    error.chars().take(MAX_ERROR_LEN).collect()
}

#[cfg(test)]
mod tests {
    use crate::crypto::envelope::encrypt_bytes;

    use super::{parse_email_payload, retry_delay_seconds, truncate_error};

    #[test]
    fn email_payload_decrypts_body() {
        let envelope = encrypt_bytes(&[4_u8; 32], b"hello email").expect("encrypt");
        let payload = serde_json::json!({
            "to": "user@example.com",
            "subject": "Hello",
            "body_envelope": envelope,
        });
        let email = parse_email_payload(&payload, &[4_u8; 32]).expect("parse");

        assert_eq!(email.to, "user@example.com");
        assert_eq!(email.subject, "Hello");
        assert_eq!(email.body, "hello email");
    }

    #[test]
    fn retry_delay_uses_bounded_exponential_backoff() {
        assert_eq!(retry_delay_seconds(1), 30);
        assert_eq!(retry_delay_seconds(2), 60);
        assert_eq!(retry_delay_seconds(20), 960);
    }

    #[test]
    fn truncate_error_limits_long_messages() {
        let long = "x".repeat(3_000);

        assert_eq!(truncate_error("short"), "short");
        assert_eq!(truncate_error(&long).len(), 2_000);
    }
}
