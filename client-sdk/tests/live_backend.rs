use std::{
    env,
    error::Error,
    time::{SystemTime, UNIX_EPOCH},
};

use client_sdk::{
    access_token::verify_access_token_with_jwks_refresh,
    auth::{ClientBootstrap, HeartbeatResponse},
    cache::SdkCacheEnvelope,
    client::{ai_models_request, ProtectedClientRequestContext},
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

#[test]
#[ignore = "requires a running backend and SDK_SMOKE_* customer login environment variables"]
fn live_backend_customer_login_ai_subscription_gate() -> Result<(), Box<dyn Error>> {
    let backend_url = required_env("SDK_SMOKE_BACKEND_URL")?
        .trim_end_matches('/')
        .to_owned();
    let app_key = required_env("SDK_SMOKE_APP_KEY")?;
    let email = required_env("SDK_SMOKE_CUSTOMER_EMAIL")?;
    let password = required_env("SDK_SMOKE_CUSTOMER_PASSWORD")?;
    let expect_subscription = required_env("SDK_SMOKE_AI_EXPECT_SUBSCRIPTION")?
        .parse::<bool>()
        .map_err(|_| "SDK_SMOKE_AI_EXPECT_SUBSCRIPTION must be true or false")?;
    let machine_id = env::var("SDK_SMOKE_MACHINE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("sdk-ai-smoke-{}", now_unix()));

    let client = Client::builder().build()?;
    let device = DeviceIdentity::generate(&app_key, &[machine_id.as_str()])?;
    let bootstrap = ClientBootstrap::new(device.clone())?;
    let login_payload = bootstrap.customer_login_request(
        &app_key,
        &email,
        &password,
        Some("SDK AI Smoke Device"),
        Some(std::env::consts::OS),
        Some("sdk-ai-smoke"),
    )?;
    let login_response = post_json(
        &client,
        &backend_url,
        "/api/client/auth/login",
        serde_json::to_vec(&login_payload)?,
        &[],
    )?;
    let login = ClientAuthSessionResponse::from_api_response_json(&login_response)?;
    assert_eq!(login.entitlement_active, expect_subscription);
    if expect_subscription {
        assert_eq!(login.entitlement_kind.as_deref(), Some("subscription"));
        assert!(login.subscription_id.is_some());
    } else {
        assert!(!login.entitlement_active);
        assert!(login.subscription_id.is_none());
    }

    let device_key_id = login
        .device_key_id
        .clone()
        .ok_or("login response did not include device_key_id")?;
    let session = bootstrap.apply_auth_response(login, now_unix())?;
    let cache = SdkCacheEnvelope::new_with_device_key_id(
        &app_key,
        device,
        Some(&device_key_id),
        Some(session),
        &JwksCache::default(),
        now_unix(),
    )?;
    let session_manager = cache.session_manager();
    let ai_nonce = format!("sdkaismoke{}", now_unix());
    let ai_request = ai_models_request(
        ProtectedClientRequestContext {
            cache: &cache,
            session_manager: &session_manager,
            timestamp: now_unix(),
            nonce: &ai_nonce,
            refresh_before_seconds: 60,
        },
        |_| unreachable!("fresh login access token should not refresh"),
    )?;
    let ai_response = get_raw(&client, &backend_url, &ai_request.path, &ai_request.headers)?;

    if expect_subscription {
        if ai_response.0 != 200 {
            return Err(format!(
                "expected subscribed AI models request to succeed, got HTTP {}: {}",
                ai_response.0, ai_response.1
            )
            .into());
        }
    } else {
        let envelope: serde_json::Value = serde_json::from_str(&ai_response.1)?;
        if ai_response.0 != 403
            || envelope.get("code").and_then(|value| value.as_i64()) != Some(40306)
            || envelope.get("message").and_then(|value| value.as_str())
                != Some("subscription_inactive")
        {
            return Err(format!(
                "expected unsubscribed AI models request to be subscription_inactive, got HTTP {}: {}",
                ai_response.0, ai_response.1
            )
            .into());
        }
    }

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

fn get_raw(
    client: &Client,
    backend_url: &str,
    path: &str,
    headers: &[(String, String)],
) -> Result<(u16, String), Box<dyn Error>> {
    let url = format!("{backend_url}{path}");
    let mut request = client.get(url);
    for (name, value) in headers {
        request = request.header(name, value);
    }

    let response = request.send()?;
    let status = response.status().as_u16();
    let text = response.text()?;

    Ok((status, text))
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_secs() as i64
}
