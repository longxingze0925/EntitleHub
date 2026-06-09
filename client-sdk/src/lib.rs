pub mod access_token;
pub mod ai;
pub mod auth;
pub mod cache;
pub mod client;
pub mod device;
pub mod error;
pub mod jwks;
pub mod request;
pub mod response;
pub mod script;
pub mod session;
pub mod signing;
pub mod update;

pub use error::{SdkError, SdkResult};

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use sha2::{Digest, Sha256};

    use crate::{
        ai::{image_urls_from_response, AiGatewayJsonResponse, AiModelListResponse},
        auth::{ClientBootstrap, HeartbeatResponse, LogoutResponse, VerifyResponse},
        cache::{LogoutClearOptions, SdkCacheEnvelope},
        client::{ai_chat_completions_request, ai_models_request, ProtectedClientRequestContext},
        device::{build_rotate_device_key_request, DeviceIdentity, RotateDeviceKeyResponse},
        jwks::JwksCache,
        request::{build_authorized_cached_device_request, CachedAuthorizedDeviceRequestInput},
        script::ScriptPackage,
        session::ClientAuthSessionResponse,
        update::UpdateInfo,
    };

    #[test]
    fn recommended_client_flow_uses_wrapped_responses_cache_and_signed_requests() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let bootstrap = ClientBootstrap::new(device.clone()).expect("bootstrap");
        let activation = bootstrap
            .activation_request(
                "app_key",
                "license",
                Some("Workstation"),
                Some("Windows"),
                Some("1.0.0"),
            )
            .expect("activation request");
        assert_eq!(activation.machine_id, device.machine_id);

        let auth_response = ClientAuthSessionResponse::from_api_response_json(
            r#"{
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
            }"#,
        )
        .expect("auth response");
        let device_key_id = auth_response
            .device_key_id
            .clone()
            .expect("device key id should be present");
        let session = bootstrap
            .apply_auth_response(auth_response, 100)
            .expect("session should apply");
        let mut cache = SdkCacheEnvelope::new_with_device_key_id(
            "app_key",
            device.clone(),
            Some(&device_key_id),
            Some(session),
            &JwksCache::default(),
            100,
        )
        .expect("cache should build");

        let heartbeat_body = br#"{"app_version":"1.0.0"}"#;
        let heartbeat_request = build_authorized_cached_device_request(
            &cache,
            &bootstrap.session_manager,
            CachedAuthorizedDeviceRequestInput {
                method: "post",
                path: "/api/client/auth/heartbeat",
                body: heartbeat_body,
                timestamp: 200,
                nonce: "0123456789abcdef",
                refresh_before_seconds: 60,
            },
            |_| unreachable!("access token should not refresh"),
        )
        .expect("heartbeat request should sign");
        assert_eq!(
            heartbeat_request.header("X-Device-Key-Id"),
            Some("device-key-id")
        );

        let heartbeat = HeartbeatResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "status": "ok",
                "server_time": 1710000000,
                "license_status": "active"
              },
              "request_id": "req_heartbeat"
            }"#,
        )
        .expect("heartbeat response");
        assert_eq!(heartbeat.license_status, "active");

        let verify = VerifyResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "valid": true,
                "features": { "tier": "pro" },
                "expires_at": "2027-01-01T00:00:00Z"
              },
              "request_id": "req_verify"
            }"#,
        )
        .expect("verify response");
        assert!(verify.valid);

        let update_json = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "app_id": "00000000-0000-0000-0000-000000000001",
                "version": "1.0.1",
                "version_code": 101,
                "download_url": "/api/client/releases/download/app.zip?token=token",
                "file_size": 123,
                "sha256": "a".repeat(64),
                "published_at_unix": 1780000000,
                "signature_kid": "release-kid",
                "signature": "signature",
                "signature_alg": "Ed25519",
                "force_update": false
            },
            "request_id": "req_update"
        })
        .to_string();
        let update = UpdateInfo::from_api_response_json(&update_json).expect("update response");
        assert_eq!(update.version_code, 101);

        let script_content = b"print('ok')";
        let script_json = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "script_id": "00000000-0000-0000-0000-000000000010",
                "version": "1.0.0",
                "version_code": 100,
                "content_base64": STANDARD.encode(script_content),
                "sha256": format!("{:x}", Sha256::digest(script_content)),
                "signature_kid": "script-kid",
                "signature": "signature",
                "signature_alg": "Ed25519",
                "expires_at_unix": 1780000000
            },
            "request_id": "req_script"
        })
        .to_string();
        let script = ScriptPackage::from_api_response_json(&script_json).expect("script response");
        assert_eq!(script.version_code, 100);

        let context = ProtectedClientRequestContext {
            cache: &cache,
            session_manager: &bootstrap.session_manager,
            timestamp: 204,
            nonce: "0123456789abcdef",
            refresh_before_seconds: 60,
        };
        let ai_models_request =
            ai_models_request(context, |_| unreachable!("access token should not refresh"))
                .expect("ai models request should sign");
        assert_eq!(ai_models_request.path, "/api/client/ai/v1/models");
        assert_eq!(
            ai_models_request.header("X-Device-Key-Id"),
            Some("device-key-id")
        );
        let ai_chat_request = ai_chat_completions_request(
            context,
            &serde_json::json!({
                "model": "gpt-test",
                "messages": [{ "role": "user", "content": "hello" }]
            }),
            Some("sdk-flow-1"),
            |_| unreachable!("access token should not refresh"),
        )
        .expect("ai chat request should sign");
        assert_eq!(
            ai_chat_request.header("Idempotency-Key"),
            Some("sdk-flow-1")
        );

        let models = AiModelListResponse::from_json(
            r#"{
              "object": "list",
              "data": [{
                "id": "gpt-test",
                "object": "model",
                "created": 1710000000,
                "owned_by": "entitlehub"
              }]
            }"#,
        )
        .expect("models response");
        assert_eq!(models.data[0].id, "gpt-test");
        let ai_response = AiGatewayJsonResponse::from_json_with_usage_id(
            r#"{
              "created": 1710000000,
              "data": [{ "url": "/api/ai/assets/00000000-0000-0000-0000-000000000099" }]
            }"#,
            Some("usage-id"),
        )
        .expect("ai response");
        assert_eq!(ai_response.usage_id.as_deref(), Some("usage-id"));
        assert_eq!(
            image_urls_from_response(&ai_response.body),
            vec!["/api/ai/assets/00000000-0000-0000-0000-000000000099".to_owned()]
        );

        let rotated_device = cache.device.rotate_key().expect("rotated key");
        let rotate_payload =
            build_rotate_device_key_request(&rotated_device).expect("rotate request payload");
        assert_eq!(
            rotate_payload.device_public_key,
            rotated_device.device_public_key
        );
        let rotate_request = build_authorized_cached_device_request(
            &cache,
            &bootstrap.session_manager,
            CachedAuthorizedDeviceRequestInput {
                method: "post",
                path: "/api/client/devices/self/rotate-key",
                body: br#"{"device_public_key":"next"}"#,
                timestamp: 201,
                nonce: "0123456789abcdeg",
                refresh_before_seconds: 60,
            },
            |_| unreachable!("access token should not refresh"),
        )
        .expect("rotate request should sign with old key");
        assert_eq!(
            rotate_request.header("X-Device-Key-Id"),
            Some("device-key-id")
        );

        let rotate_response_json = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "device_key_id": "next-device-key-id",
                "device_public_key": rotated_device.device_public_key,
                "algorithm": "Ed25519",
                "status": "active",
                "rotated_device_key_ids": ["device-key-id"]
            },
            "request_id": "req_rotate"
        })
        .to_string();
        let rotate_response =
            RotateDeviceKeyResponse::from_api_response_json(&rotate_response_json)
                .expect("rotate response");
        cache
            .apply_device_key_rotation(rotated_device, &rotate_response.device_key_id, 202)
            .expect("rotation should update cache");
        let signed_after_rotation = build_authorized_cached_device_request(
            &cache,
            &bootstrap.session_manager,
            CachedAuthorizedDeviceRequestInput {
                method: "post",
                path: "/api/client/auth/verify",
                body: b"",
                timestamp: 203,
                nonce: "0123456789abcdef",
                refresh_before_seconds: 60,
            },
            |_| unreachable!("access token should not refresh"),
        )
        .expect("request should sign with new key");
        assert_eq!(
            signed_after_rotation.header("X-Device-Key-Id"),
            Some("next-device-key-id")
        );

        let logout = LogoutResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": { "revoked": true },
              "request_id": "req_logout"
            }"#,
        )
        .expect("logout response");
        assert!(logout.revoked);

        let logout_cache = cache
            .into_logout_cache(LogoutClearOptions::default(), 204)
            .expect("logout cache should build")
            .expect("device identity should remain");
        assert!(logout_cache.session.is_none());
        assert_eq!(
            logout_cache.device_key_id.as_deref(),
            Some("next-device-key-id")
        );
    }
}
