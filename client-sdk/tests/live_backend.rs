use std::{
    env,
    error::Error,
    time::{SystemTime, UNIX_EPOCH},
};

use client_sdk::{
    access_token::verify_access_token_with_jwks_refresh,
    auth::{ClientBootstrap, HeartbeatResponse},
    cache::SdkCacheEnvelope,
    device::DeviceIdentity,
    jwks::JwksCache,
    request::{build_authorized_cached_device_request, CachedAuthorizedDeviceRequestInput},
    session::ClientAuthSessionResponse,
};
use reqwest::blocking::Client;
use serde_json::json;

#[test]
#[ignore = "requires a running backend and SDK_SMOKE_* environment variables"]
fn live_backend_activation_refresh_and_heartbeat() -> Result<(), Box<dyn Error>> {
    let backend_url = required_env("SDK_SMOKE_BACKEND_URL")?
        .trim_end_matches('/')
        .to_owned();
    let app_key = required_env("SDK_SMOKE_APP_KEY")?;
    let license_key = required_env("SDK_SMOKE_LICENSE_KEY")?;
    let machine_id = env::var("SDK_SMOKE_MACHINE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("sdk-smoke-{}", now_unix()));
    let jwt_issuer = required_env("SDK_SMOKE_JWT_ISSUER")?;
    let jwt_audience =
        env::var("SDK_SMOKE_JWT_AUDIENCE").unwrap_or_else(|_| "client-sdk".to_owned());

    let client = Client::builder().build()?;
    let device = DeviceIdentity::generate(&app_key, &[machine_id.as_str()])?;
    let bootstrap = ClientBootstrap::new(device.clone())?;
    let activation_payload = bootstrap.activation_request(
        &app_key,
        &license_key,
        Some("SDK Smoke Device"),
        Some(std::env::consts::OS),
        Some("sdk-smoke"),
    )?;
    let activation_body = serde_json::to_vec(&activation_payload)?;
    let activation_response = post_json(
        &client,
        &backend_url,
        "/api/client/auth/activate",
        activation_body,
        &[],
    )?;
    let activation = ClientAuthSessionResponse::from_api_response_json(&activation_response)?;
    let device_key_id = activation
        .device_key_id
        .clone()
        .ok_or("activation response did not include device_key_id")?;
    let mut session = bootstrap.apply_auth_response(activation, now_unix())?;

    let refresh_body = serde_json::to_vec(&json!({
        "refresh_token": session.refresh_token,
    }))?;
    let refresh_response = post_json(
        &client,
        &backend_url,
        "/api/client/auth/refresh",
        refresh_body,
        &[],
    )?;
    let refresh = ClientAuthSessionResponse::from_api_response_json(&refresh_response)?;
    session.apply_refresh(refresh.into_session_refresh(), now_unix())?;

    let mut jwks_cache = JwksCache::default();
    let claims = verify_access_token_with_jwks_refresh(
        &session.access_token,
        &mut jwks_cache,
        &jwt_issuer,
        &jwt_audience,
        now_unix(),
        || {
            get_text(&client, &backend_url, "/.well-known/jwks.json")
                .map_err(|_| client_sdk::SdkError::InvalidJwks)
        },
    )?;
    assert_eq!(claims.device_id, session.device_id);
    assert_eq!(claims.machine_id, device.machine_id);

    let cache = SdkCacheEnvelope::new_with_device_key_id(
        &app_key,
        device,
        Some(&device_key_id),
        Some(session),
        &jwks_cache,
        now_unix(),
    )?;
    let session_manager = cache.session_manager();
    let heartbeat_body = br#"{"app_version":"sdk-smoke"}"#;
    let heartbeat_headers = build_authorized_cached_device_request(
        &cache,
        &session_manager,
        CachedAuthorizedDeviceRequestInput {
            method: "post",
            path: "/api/client/auth/heartbeat",
            body: heartbeat_body,
            timestamp: now_unix(),
            nonce: &format!("sdksmoke{}", now_unix()),
            refresh_before_seconds: 60,
        },
        |_| unreachable!("freshly refreshed access token should not refresh"),
    )?;
    let heartbeat_response = post_json(
        &client,
        &backend_url,
        "/api/client/auth/heartbeat",
        heartbeat_body.to_vec(),
        &heartbeat_headers.headers,
    )?;
    let heartbeat = HeartbeatResponse::from_api_response_json(&heartbeat_response)?;
    assert_eq!(heartbeat.status, "ok");
    assert_eq!(heartbeat.license_status, "active");

    Ok(())
}

fn required_env(name: &str) -> Result<String, Box<dyn Error>> {
    let value = env::var(name)?;
    if value.trim().is_empty() {
        return Err(format!("{name} must not be blank").into());
    }

    Ok(value)
}

fn post_json(
    client: &Client,
    backend_url: &str,
    path: &str,
    body: Vec<u8>,
    headers: &[(String, String)],
) -> Result<String, Box<dyn Error>> {
    let url = format!("{backend_url}{path}");
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .body(body);
    for (name, value) in headers {
        request = request.header(name, value);
    }

    let response = request.send()?;
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(format!("{path} returned HTTP {status}: {text}").into());
    }

    Ok(text)
}

fn get_text(client: &Client, backend_url: &str, path: &str) -> Result<String, Box<dyn Error>> {
    let url = format!("{backend_url}{path}");
    let response = client.get(url).send()?;
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        return Err(format!("{path} returned HTTP {status}: {text}").into());
    }

    Ok(text)
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs() as i64
}
