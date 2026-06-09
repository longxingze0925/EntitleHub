use std::error::Error;

use client_sdk::{
    auth::{ClientBootstrap, HeartbeatResponse, LogoutResponse},
    cache::{LogoutClearOptions, SdkCacheEnvelope},
    client::{
        ai_chat_completions_request, heartbeat_request, rotate_device_key_request,
        ProtectedClientRequestContext,
    },
    device::{DeviceIdentity, RotateDeviceKeyResponse},
    jwks::JwksCache,
    session::{ClientAuthSessionResponse, ClientSessionState, SessionRefresh},
    signing::generate_device_nonce,
};

fn main() -> Result<(), Box<dyn Error>> {
    let now_unix = 1_717_171_717;
    let device = DeviceIdentity::generate("app_key", &["machine-fingerprint"])?;
    let bootstrap = ClientBootstrap::new(device.clone())?;

    let activation_payload = bootstrap.activation_request(
        "app_key",
        "license_key",
        Some("Workstation"),
        Some("Windows"),
        Some("1.0.0"),
    )?;
    let activation_body = serde_json::to_vec(&activation_payload)?;
    println!("activation body bytes: {}", activation_body.len());

    let auth_response_json = serde_json::json!({
        "code": 0,
        "message": "ok",
        "data": {
            "access_token": "access-token",
            "refresh_token": "refresh-token",
            "token_type": "Bearer",
            "expires_in": 900,
            "refresh_expires_in": 2500,
            "session_id": "session-id",
            "device_id": "device-id",
            "device_key_id": "device-key-id",
            "features": { "tier": "pro" }
        },
        "request_id": "req_auth"
    })
    .to_string();
    let auth = ClientAuthSessionResponse::from_api_response_json(&auth_response_json)?;
    let device_key_id = auth.device_key_id.clone();
    let session = bootstrap.apply_auth_response(auth, now_unix)?;
    let mut cache = SdkCacheEnvelope::new_with_device_key_id(
        "app_key",
        device,
        device_key_id.as_deref(),
        Some(session),
        &JwksCache::default(),
        now_unix,
    )?;
    let persisted_cache_json = cache.to_json()?;
    println!("cache bytes: {}", persisted_cache_json.len());

    let session_manager = cache.session_manager();
    let heartbeat_nonce = generate_device_nonce()?;
    let heartbeat_context = ProtectedClientRequestContext {
        cache: &cache,
        session_manager: &session_manager,
        timestamp: now_unix + 1,
        nonce: &heartbeat_nonce,
        refresh_before_seconds: 60,
    };
    let heartbeat_request = heartbeat_request(heartbeat_context, Some("1.0.0"), refresh_session)?;
    println!("heartbeat headers: {}", heartbeat_request.headers.len());

    let heartbeat = HeartbeatResponse::from_api_response_json(
        r#"{
          "code": 0,
          "message": "ok",
          "data": {
            "status": "ok",
            "server_time": 1717171718,
            "license_status": "active"
          },
          "request_id": "req_heartbeat"
        }"#,
    )?;
    println!("heartbeat license status: {}", heartbeat.license_status);

    let next_device = cache.device.rotate_key()?;
    let rotate_nonce = generate_device_nonce()?;
    let rotate_context = ProtectedClientRequestContext {
        cache: &cache,
        session_manager: &session_manager,
        timestamp: now_unix + 2,
        nonce: &rotate_nonce,
        refresh_before_seconds: 60,
    };
    let rotate_request = rotate_device_key_request(rotate_context, &next_device, refresh_session)?;
    println!(
        "rotate request device key id: {}",
        rotate_request.header("X-Device-Key-Id").unwrap_or("")
    );

    let rotate_response_json = serde_json::json!({
        "code": 0,
        "message": "ok",
        "data": {
            "device_key_id": "next-device-key-id",
            "device_public_key": next_device.device_public_key,
            "algorithm": "Ed25519",
            "status": "active",
            "rotated_device_key_ids": ["device-key-id"]
        },
        "request_id": "req_rotate"
    })
    .to_string();
    let rotate = RotateDeviceKeyResponse::from_api_response_json(&rotate_response_json)?;
    cache.apply_device_key_rotation(next_device, &rotate.device_key_id, now_unix + 3)?;
    println!(
        "cache now uses device key id: {}",
        cache.device_key_id.as_deref().unwrap_or("")
    );

    let ai_nonce = generate_device_nonce()?;
    let ai_context = ProtectedClientRequestContext {
        cache: &cache,
        session_manager: &session_manager,
        timestamp: now_unix + 4,
        nonce: &ai_nonce,
        refresh_before_seconds: 60,
    };
    let ai_request = ai_chat_completions_request(
        ai_context,
        &serde_json::json!({
            "model": "gpt-test",
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        Some("demo-request-1"),
        refresh_session,
    )?;
    println!("ai request path: {}", ai_request.path);

    let logout = LogoutResponse::from_api_response_json(
        r#"{
          "code": 0,
          "message": "ok",
          "data": { "revoked": true },
          "request_id": "req_logout"
        }"#,
    )?;
    println!("logout revoked: {}", logout.revoked);

    let logout_cache = cache.into_logout_cache(LogoutClearOptions::default(), now_unix + 4)?;
    println!("cache kept after logout: {}", logout_cache.is_some());

    Ok(())
}

fn refresh_session(_session: &ClientSessionState) -> client_sdk::SdkResult<SessionRefresh> {
    Ok(SessionRefresh {
        access_token: "next-access-token".to_owned(),
        refresh_token: "next-refresh-token".to_owned(),
        token_type: Some("Bearer".to_owned()),
        expires_in: 900,
        refresh_expires_in: 2_500,
        features: serde_json::json!({ "tier": "pro" }),
    })
}
