use axum::http::HeaderMap;
use redis::AsyncCommands;

use crate::{error::AppError, metrics, state::AppState};

pub const INTERNAL_CLIENT_IP_HEADER: &str = "x-internal-client-ip";

pub async fn check_fixed_window(
    state: &AppState,
    key: String,
    max_attempts: u32,
    window_seconds: u64,
    limited_error: fn() -> AppError,
) -> Result<(), AppError> {
    if max_attempts == 0 {
        return Err(limited_error());
    }
    if window_seconds == 0 {
        return Err(AppError::config(
            "rate limit window_seconds must be greater than 0",
        ));
    }

    let mut connection = state
        .redis
        .get_multiplexed_async_connection()
        .await
        .map_err(|error| {
            metrics::record_redis_error();
            AppError::dependency(format!("redis connection failed: {error}"))
        })?;
    let count: u32 = connection.incr(&key, 1_u32).await.map_err(|error| {
        metrics::record_redis_error();
        AppError::dependency(format!("redis rate limit incr failed: {error}"))
    })?;
    if count == 1 {
        let _: bool = connection
            .expire(&key, window_seconds as i64)
            .await
            .map_err(|error| {
                metrics::record_redis_error();
                AppError::dependency(format!("redis rate limit expire failed: {error}"))
            })?;
    }
    if count > max_attempts {
        return Err(limited_error());
    }

    Ok(())
}

pub async fn check_client_action(
    state: &AppState,
    action: &str,
    device_id: &str,
) -> Result<(), AppError> {
    check_fixed_window(
        state,
        client_action_key(action, device_id),
        state.config.security.client_action_rate_limit_max,
        state
            .config
            .security
            .client_action_rate_limit_window_seconds,
        AppError::rate_limited,
    )
    .await
}

pub fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get(INTERNAL_CLIENT_IP_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

pub fn login_key(email: &str, ip: &str) -> String {
    format!(
        "rate:login:{}:{}",
        key_part(&email.trim().to_lowercase()),
        key_part(ip)
    )
}

pub fn activation_key(app_key: &str, machine_id: &str, ip: &str) -> String {
    format!(
        "rate:activation:{}:{}:{}",
        key_part(app_key.trim()),
        key_part(machine_id.trim()),
        key_part(ip)
    )
}

pub fn refresh_key(session_id: &str, ip: &str) -> String {
    format!("rate:refresh:{}:{}", key_part(session_id), key_part(ip))
}

pub fn heartbeat_key(device_id: &str) -> String {
    format!("rate:heartbeat:{}", key_part(device_id))
}

pub fn client_action_key(action: &str, device_id: &str) -> String {
    format!(
        "rate:client_action:{}:{}",
        key_part(action),
        key_part(device_id)
    )
}

pub fn email_verify_key(subject_id: &str, ip: &str) -> String {
    format!(
        "rate:email_verify:{}:{}",
        key_part(subject_id),
        key_part(ip)
    )
}

pub fn mfa_key(subject_id: &str, ip: &str) -> String {
    format!("rate:mfa:{}:{}", key_part(subject_id), key_part(ip))
}

pub fn download_key(file_id: &str, device_id: &str) -> String {
    format!(
        "rate:download:{}:{}",
        key_part(file_id),
        key_part(device_id)
    )
}

pub fn ai_gateway_key(api_key_id: &str) -> String {
    format!("rate:ai_gateway:{}", key_part(api_key_id))
}

fn key_part(value: &str) -> String {
    let normalized = value.trim();
    if normalized.is_empty() {
        return "empty".to_owned();
    }

    normalized
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | '@' => character,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{
        activation_key, ai_gateway_key, client_action_key, client_ip, download_key,
        email_verify_key, heartbeat_key, key_part, login_key, mfa_key, refresh_key,
        INTERNAL_CLIENT_IP_HEADER,
    };

    #[test]
    fn client_ip_uses_internal_resolved_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            INTERNAL_CLIENT_IP_HEADER,
            HeaderValue::from_static("203.0.113.10"),
        );
        headers.insert("X-Real-IP", HeaderValue::from_static("198.51.100.7"));

        assert_eq!(client_ip(&headers), "203.0.113.10");
    }

    #[test]
    fn client_ip_is_unknown_without_internal_header() {
        let headers = HeaderMap::new();

        assert_eq!(client_ip(&headers), "unknown");
    }

    #[test]
    fn key_part_replaces_unsafe_characters() {
        assert_eq!(key_part(" app:key / one "), "app_key___one");
        assert_eq!(key_part(" "), "empty");
    }

    #[test]
    fn keys_are_stable_and_scoped() {
        assert_eq!(
            login_key(" User@Example.COM ", "203.0.113.10"),
            "rate:login:user@example.com:203.0.113.10"
        );
        assert_eq!(
            activation_key("app-1", "machine/id", "203.0.113.10"),
            "rate:activation:app-1:machine_id:203.0.113.10"
        );
        assert_eq!(
            refresh_key("session-id", "203.0.113.10"),
            "rate:refresh:session-id:203.0.113.10"
        );
        assert_eq!(heartbeat_key("device-id"), "rate:heartbeat:device-id");
        assert_eq!(
            client_action_key("script/fetch", "device/id"),
            "rate:client_action:script_fetch:device_id"
        );
        assert_eq!(
            email_verify_key("customer/id", "203.0.113.10"),
            "rate:email_verify:customer_id:203.0.113.10"
        );
        assert_eq!(
            mfa_key("member/id", "203.0.113.10"),
            "rate:mfa:member_id:203.0.113.10"
        );
        assert_eq!(
            download_key("file-id", "device-id"),
            "rate:download:file-id:device-id"
        );
        assert_eq!(ai_gateway_key("api/key-id"), "rate:ai_gateway:api_key-id");
    }
}
