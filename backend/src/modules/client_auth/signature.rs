use axum::{
    body::{to_bytes, Body},
    extract::{Request, State},
    http::HeaderMap,
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use redis::AsyncCommands;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    crypto::signing::verify_ed25519_signature,
    error::AppError,
    metrics,
    modules::{client_auth::session::ClientContext, device::repository::DeviceRepository},
    state::AppState,
};

const NONCE_TTL_SECONDS: u64 = 300;
const TIMESTAMP_TOLERANCE_SECONDS: i64 = 300;
const NONCE_MIN_LEN: usize = 16;
const NONCE_MAX_LEN: usize = 128;

pub async fn require_device_signature(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let client = request
        .extensions()
        .get::<ClientContext>()
        .cloned()
        .ok_or_else(|| AppError::unauthenticated())?;
    let headers = request.headers().clone();
    let device_key_id = read_uuid_header(&headers, "X-Device-Key-Id")?;
    let timestamp = read_i64_header(&headers, "X-Timestamp")?;
    let nonce = read_nonce_header(&headers, "X-Nonce")?;
    let body_sha256 = read_required_header(&headers, "X-Body-SHA256")?;
    let signature = read_required_header(&headers, "X-Signature")?;

    let now = Utc::now().timestamp();
    if (now - timestamp).abs() > TIMESTAMP_TOLERANCE_SECONDS {
        return Err(AppError::signature_invalid("signature timestamp expired"));
    }

    let (parts, body) = request.into_parts();
    let body_bytes = to_bytes(body, 1024 * 1024)
        .await
        .map_err(|_| AppError::signature_invalid("request body invalid"))?;
    validate_body_hash(&body_bytes, body_sha256)?;

    let device_key = DeviceRepository::new(state.db.clone())
        .find_active_key(
            client.tenant_id,
            client.app_id,
            client.device_id,
            device_key_id,
        )
        .await?
        .ok_or_else(|| AppError::signature_invalid("device key invalid"))?;
    let message = signature_message(
        parts.method.as_str(),
        parts.uri.path(),
        body_sha256,
        timestamp,
        nonce,
        client.device_id,
        device_key_id,
        client.session_id,
    );
    verify_ed25519_signature(&device_key.public_key, message.as_bytes(), signature)
        .map_err(|_| AppError::signature_invalid("signature invalid"))?;
    store_nonce(&state, &client, nonce).await?;

    let request = Request::from_parts(parts, Body::from(body_bytes));
    Ok(next.run(request).await)
}

fn signature_message(
    method: &str,
    path: &str,
    body_sha256: &str,
    timestamp: i64,
    nonce: &str,
    device_id: Uuid,
    device_key_id: Uuid,
    session_id: Uuid,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.to_uppercase(),
        path,
        body_sha256,
        timestamp,
        nonce,
        device_id,
        device_key_id,
        session_id
    )
}

fn validate_body_hash(body: &[u8], expected_hash: &str) -> Result<(), AppError> {
    let actual_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(body));
    if actual_hash != expected_hash {
        return Err(AppError::signature_invalid("body hash mismatch"));
    }

    Ok(())
}

async fn store_nonce(
    state: &AppState,
    client: &ClientContext,
    nonce: &str,
) -> Result<(), AppError> {
    let mut connection = state
        .redis
        .get_multiplexed_async_connection()
        .await
        .map_err(|error| {
            metrics::record_redis_error();
            AppError::dependency(format!("redis connection failed: {error}"))
        })?;
    let key = format!(
        "nonce:{}:{}:{}:{}",
        client.tenant_id, client.app_id, client.device_id, nonce
    );
    let stored: bool = connection.set_nx(&key, "1").await.map_err(|error| {
        metrics::record_redis_error();
        AppError::dependency(format!("redis nonce set failed: {error}"))
    })?;
    if !stored {
        metrics::record_nonce_replay();
        return Err(AppError::signature_invalid("nonce already used"));
    }
    let _: bool = connection
        .expire(&key, NONCE_TTL_SECONDS as i64)
        .await
        .map_err(|error| {
            metrics::record_redis_error();
            AppError::dependency(format!("redis nonce expire failed: {error}"))
        })?;

    Ok(())
}

fn read_required_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, AppError> {
    let value = headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::signature_required(format!("{name} is required")))?;

    Ok(value)
}

fn read_uuid_header(headers: &HeaderMap, name: &str) -> Result<Uuid, AppError> {
    read_required_header(headers, name)?
        .parse()
        .map_err(|_| AppError::signature_invalid(format!("{name} is invalid")))
}

fn read_i64_header(headers: &HeaderMap, name: &str) -> Result<i64, AppError> {
    read_required_header(headers, name)?
        .parse()
        .map_err(|_| AppError::signature_invalid(format!("{name} is invalid")))
}

fn read_nonce_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, AppError> {
    let nonce = read_required_header(headers, name)?;
    if nonce.len() < NONCE_MIN_LEN
        || nonce.len() > NONCE_MAX_LEN
        || !nonce
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AppError::signature_invalid(format!("{name} is invalid")));
    }

    Ok(nonce)
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;
    use base64::Engine as _;
    use sha2::Digest as _;
    use uuid::Uuid;

    use super::{read_nonce_header, signature_message, validate_body_hash};

    #[test]
    fn body_hash_uses_urlsafe_sha256() {
        let body = br#"{"a":1}"#;
        let hash =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(body));

        assert!(validate_body_hash(body, &hash).is_ok());
        assert!(validate_body_hash(body, "bad").is_err());
    }

    #[test]
    fn signature_message_uses_expected_order() {
        let message = signature_message(
            "post",
            "/api/client/auth/verify",
            "hash",
            123,
            "nonce",
            Uuid::nil(),
            Uuid::nil(),
            Uuid::nil(),
        );

        assert!(message.starts_with("POST\n/api/client/auth/verify\nhash\n123\nnonce"));
    }

    #[test]
    fn nonce_header_rejects_short_long_or_unsafe_values() {
        let mut headers = HeaderMap::new();
        headers.insert("X-Nonce", "0123456789abcdef".parse().expect("header"));
        assert!(read_nonce_header(&headers, "X-Nonce").is_ok());

        headers.insert("X-Nonce", "short".parse().expect("header"));
        assert!(read_nonce_header(&headers, "X-Nonce").is_err());

        headers.insert("X-Nonce", "bad:nonce:value".parse().expect("header"));
        assert!(read_nonce_header(&headers, "X-Nonce").is_err());

        headers.insert("X-Nonce", "a".repeat(129).parse().expect("header"));
        assert!(read_nonce_header(&headers, "X-Nonce").is_err());
    }
}
