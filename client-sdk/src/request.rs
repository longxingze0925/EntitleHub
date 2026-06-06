use crate::{
    cache::SdkCacheEnvelope,
    session::{ClientSessionState, SessionManager, SessionRefresh},
    signing::{sign_device_request, DeviceSignatureHeaders, DeviceSignatureInput},
    SdkError, SdkResult,
};

#[derive(Debug, Clone)]
pub struct AuthorizedDeviceRequestInput<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub timestamp: i64,
    pub nonce: &'a str,
    pub refresh_before_seconds: i64,
    pub device_id: &'a str,
    pub device_key_id: &'a str,
    pub private_key_pkcs8_der: &'a [u8],
}

#[derive(Debug, Clone)]
pub struct CachedAuthorizedDeviceRequestInput<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub body: &'a [u8],
    pub timestamp: i64,
    pub nonce: &'a str,
    pub refresh_before_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizedRequestParts {
    pub headers: Vec<(String, String)>,
}

impl AuthorizedRequestParts {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
    }
}

pub fn build_authorized_device_request<F>(
    session_manager: &SessionManager,
    input: AuthorizedDeviceRequestInput<'_>,
    refresh: F,
) -> SdkResult<AuthorizedRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let session = session_manager.session_after_refresh_if_needed(
        input.timestamp,
        input.refresh_before_seconds,
        refresh,
    )?;
    let authorization = session.authorization_header_value()?;
    let device_headers = sign_device_request(DeviceSignatureInput {
        method: input.method,
        path: input.path,
        body: input.body,
        timestamp: input.timestamp,
        nonce: input.nonce,
        device_id: input.device_id,
        device_key_id: input.device_key_id,
        session_id: &session.session_id,
        private_key_pkcs8_der: input.private_key_pkcs8_der,
    })?;

    Ok(AuthorizedRequestParts {
        headers: request_headers(authorization, device_headers),
    })
}

pub fn build_authorized_cached_device_request<F>(
    cache: &SdkCacheEnvelope,
    session_manager: &SessionManager,
    input: CachedAuthorizedDeviceRequestInput<'_>,
    refresh: F,
) -> SdkResult<AuthorizedRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    cache.validate()?;
    let device_key_id = cache
        .device_key_id
        .as_deref()
        .ok_or(SdkError::InvalidCache)?;
    let private_key_pkcs8_der = cache.device.private_key_pkcs8_der()?;
    let session = session_manager.session_after_refresh_if_needed(
        input.timestamp,
        input.refresh_before_seconds,
        refresh,
    )?;
    let authorization = session.authorization_header_value()?;
    let device_headers = sign_device_request(DeviceSignatureInput {
        method: input.method,
        path: input.path,
        body: input.body,
        timestamp: input.timestamp,
        nonce: input.nonce,
        device_id: &session.device_id,
        device_key_id,
        session_id: &session.session_id,
        private_key_pkcs8_der: &private_key_pkcs8_der,
    })?;

    Ok(AuthorizedRequestParts {
        headers: request_headers(authorization, device_headers),
    })
}

fn request_headers(
    authorization: String,
    device_headers: DeviceSignatureHeaders,
) -> Vec<(String, String)> {
    vec![
        ("Authorization".to_owned(), authorization),
        ("X-Device-Id".to_owned(), device_headers.device_id),
        ("X-Device-Key-Id".to_owned(), device_headers.device_key_id),
        ("X-Timestamp".to_owned(), device_headers.timestamp),
        ("X-Nonce".to_owned(), device_headers.nonce),
        ("X-Body-SHA256".to_owned(), device_headers.body_sha256),
        ("X-Signature".to_owned(), device_headers.signature),
    ]
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519},
    };

    use crate::{
        cache::SdkCacheEnvelope,
        device::DeviceIdentity,
        jwks::JwksCache,
        session::{ClientSessionState, SessionInit, SessionManager, SessionRefresh},
        signing::{device_signature_message, sign_device_request, DeviceSignatureInput},
    };

    use super::{
        build_authorized_cached_device_request, build_authorized_device_request,
        AuthorizedDeviceRequestInput, CachedAuthorizedDeviceRequestInput,
    };

    #[test]
    fn authorized_request_builds_bearer_and_device_signature_headers() {
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).expect("generate key");
        let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("read key");
        let manager = SessionManager::new(Some(fixture_session(100)));
        let body = br#"{"ok":true}"#;

        let parts = build_authorized_device_request(
            &manager,
            AuthorizedDeviceRequestInput {
                method: "post",
                path: "/api/client/auth/verify",
                body,
                timestamp: 200,
                nonce: "0123456789abcdef",
                refresh_before_seconds: 60,
                device_id: "device-id",
                device_key_id: "device-key-id",
                private_key_pkcs8_der: pkcs8.as_ref(),
            },
            |_| unreachable!("access token should not need refresh"),
        )
        .expect("request parts should build");

        assert_eq!(parts.header("Authorization"), Some("Bearer access-token"));
        assert_eq!(parts.header("X-Device-Id"), Some("device-id"));
        assert_eq!(parts.header("x-device-key-id"), Some("device-key-id"));

        let message = device_signature_message(
            "post",
            "/api/client/auth/verify",
            parts.header("X-Body-SHA256").expect("body hash"),
            200,
            "0123456789abcdef",
            "device-id",
            "device-key-id",
            "session-id",
        );
        let signature = URL_SAFE_NO_PAD
            .decode(parts.header("X-Signature").expect("signature"))
            .expect("signature base64");
        let public_key = UnparsedPublicKey::new(&ED25519, key_pair.public_key().as_ref());
        public_key
            .verify(message.as_bytes(), &signature)
            .expect("signature should verify");
    }

    #[test]
    fn authorized_request_refreshes_session_before_signing() {
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).expect("generate key");
        let manager = SessionManager::new(Some(fixture_session(100)));

        let parts = build_authorized_device_request(
            &manager,
            AuthorizedDeviceRequestInput {
                method: "get",
                path: "/api/client/releases/latest",
                body: b"",
                timestamp: 940,
                nonce: "0123456789abcdef",
                refresh_before_seconds: 60,
                device_id: "device-id",
                device_key_id: "device-key-id",
                private_key_pkcs8_der: pkcs8.as_ref(),
            },
            |_| {
                Ok(SessionRefresh {
                    access_token: "next-access".to_owned(),
                    refresh_token: "next-refresh".to_owned(),
                    token_type: None,
                    expires_in: 900,
                    refresh_expires_in: 2_500,
                    features: serde_json::json!({}),
                })
            },
        )
        .expect("request parts should build");

        assert_eq!(parts.header("Authorization"), Some("Bearer next-access"));
        assert_eq!(
            manager
                .current_session()
                .expect("session should be readable")
                .expect("session should exist")
                .refresh_token,
            "next-refresh"
        );
    }

    #[test]
    fn cached_authorized_request_uses_cache_device_key_and_session_ids() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let private_key = device
            .private_key_pkcs8_der()
            .expect("private key should decode");
        let session = fixture_session(100);
        let manager = SessionManager::new(Some(session.clone()));
        let cache = SdkCacheEnvelope::new_with_device_key_id(
            "app_key",
            device.clone(),
            Some("device-key-id"),
            Some(session.clone()),
            &JwksCache::default(),
            100,
        )
        .expect("cache should build");
        let body = br#"{"ok":true}"#;

        let parts = build_authorized_cached_device_request(
            &cache,
            &manager,
            CachedAuthorizedDeviceRequestInput {
                method: "post",
                path: "/api/client/auth/verify",
                body,
                timestamp: 200,
                nonce: "0123456789abcdef",
                refresh_before_seconds: 60,
            },
            |_| unreachable!("access token should not need refresh"),
        )
        .expect("request parts should build");

        assert_eq!(parts.header("Authorization"), Some("Bearer access-token"));
        assert_eq!(parts.header("X-Device-Id"), Some("device-id"));
        assert_eq!(parts.header("X-Device-Key-Id"), Some("device-key-id"));

        let expected = sign_device_request(DeviceSignatureInput {
            method: "post",
            path: "/api/client/auth/verify",
            body,
            timestamp: 200,
            nonce: "0123456789abcdef",
            device_id: "device-id",
            device_key_id: "device-key-id",
            session_id: "session-id",
            private_key_pkcs8_der: &private_key,
        })
        .expect("expected signature should build");
        assert_eq!(
            parts.header("X-Signature"),
            Some(expected.signature.as_str())
        );
    }

    #[test]
    fn cached_authorized_request_requires_device_key_id() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let session = fixture_session(100);
        let manager = SessionManager::new(Some(session.clone()));
        let cache =
            SdkCacheEnvelope::new("app_key", device, Some(session), &JwksCache::default(), 100)
                .expect("cache should build");

        assert!(build_authorized_cached_device_request(
            &cache,
            &manager,
            CachedAuthorizedDeviceRequestInput {
                method: "get",
                path: "/api/client/releases/latest",
                body: b"",
                timestamp: 200,
                nonce: "0123456789abcdef",
                refresh_before_seconds: 60,
            },
            |_| unreachable!("request should fail before refresh"),
        )
        .is_err());
    }

    fn fixture_session(now_unix: i64) -> ClientSessionState {
        ClientSessionState::from_init(
            SessionInit {
                session_id: "session-id".to_owned(),
                device_id: "device-id".to_owned(),
                access_token: "access-token".to_owned(),
                refresh_token: "refresh-token".to_owned(),
                token_type: None,
                expires_in: 900,
                refresh_expires_in: 2_500,
                features: serde_json::json!({}),
            },
            now_unix,
        )
        .expect("session should build")
    }
}
