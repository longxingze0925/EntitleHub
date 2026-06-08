use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::error::{SdkError, SdkResult};

const DEFAULT_TOKEN_TYPE: &str = "Bearer";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientSessionState {
    pub session_id: String,
    pub device_id: String,
    pub token_type: String,
    pub access_token: String,
    pub refresh_token: String,
    pub access_token_expires_at_unix: i64,
    pub refresh_token_expires_at_unix: i64,
    pub features: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionInit {
    pub session_id: String,
    pub device_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: Option<String>,
    pub expires_in: i64,
    pub refresh_expires_in: i64,
    pub features: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRefresh {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: Option<String>,
    pub expires_in: i64,
    pub refresh_expires_in: i64,
    pub features: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClientAuthSessionResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: Option<String>,
    pub expires_in: i64,
    pub refresh_expires_in: i64,
    pub session_id: String,
    pub device_id: String,
    #[serde(default)]
    pub device_key_id: Option<String>,
    #[serde(default)]
    pub subscription_id: Option<String>,
    #[serde(default)]
    pub entitlement_id: Option<String>,
    #[serde(default)]
    pub entitlement_kind: Option<String>,
    #[serde(default)]
    pub entitlement_status: Option<String>,
    #[serde(default)]
    pub entitlement_active: bool,
    pub features: serde_json::Value,
}

impl ClientAuthSessionResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let response: Self = serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)?;
        response.validate()?;

        Ok(response)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        let response: Self = crate::response::parse_api_response_data(json)?.data;
        response.validate()?;

        Ok(response)
    }

    pub fn into_session_init(self) -> SessionInit {
        SessionInit {
            session_id: self.session_id,
            device_id: self.device_id,
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            token_type: self.token_type,
            expires_in: self.expires_in,
            refresh_expires_in: self.refresh_expires_in,
            features: self.features,
        }
    }

    pub fn into_session_refresh(self) -> SessionRefresh {
        SessionRefresh {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            token_type: self.token_type,
            expires_in: self.expires_in,
            refresh_expires_in: self.refresh_expires_in,
            features: self.features,
        }
    }

    fn validate(&self) -> SdkResult<()> {
        validate_optional_device_key_id(self.device_key_id.as_deref())
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionManager {
    session: Arc<Mutex<Option<ClientSessionState>>>,
}

impl SessionManager {
    pub fn new(session: Option<ClientSessionState>) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
        }
    }

    pub fn current_session(&self) -> SdkResult<Option<ClientSessionState>> {
        let session = self.session.lock().map_err(|_| SdkError::InvalidSession)?;

        Ok(session.clone())
    }

    pub fn set_session(&self, session: ClientSessionState) -> SdkResult<()> {
        session.validate()?;
        let mut current = self.session.lock().map_err(|_| SdkError::InvalidSession)?;
        *current = Some(session);

        Ok(())
    }

    pub fn clear_session(&self) -> SdkResult<()> {
        let mut current = self.session.lock().map_err(|_| SdkError::InvalidSession)?;
        *current = None;

        Ok(())
    }

    pub fn authorization_header_value<F>(
        &self,
        now_unix: i64,
        refresh_before_seconds: i64,
        refresh: F,
    ) -> SdkResult<String>
    where
        F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
    {
        self.session_after_refresh_if_needed(now_unix, refresh_before_seconds, refresh)?
            .authorization_header_value()
    }

    pub fn session_after_refresh_if_needed<F>(
        &self,
        now_unix: i64,
        refresh_before_seconds: i64,
        refresh: F,
    ) -> SdkResult<ClientSessionState>
    where
        F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
    {
        let mut current = self.session.lock().map_err(|_| SdkError::InvalidSession)?;
        let Some(session) = current.as_mut() else {
            return Err(SdkError::InvalidSession);
        };

        if session.needs_access_token_refresh(now_unix, refresh_before_seconds) {
            if session.is_refresh_token_expired(now_unix) {
                *current = None;
                return Err(SdkError::ExpiredRefreshToken);
            }

            let refresh_result =
                refresh(session).and_then(|next| session.apply_refresh(next, now_unix));
            if let Err(error) = refresh_result {
                *current = None;
                return Err(error);
            }
        }

        Ok(session.clone())
    }

    pub fn refresh_if_needed<F>(
        &self,
        now_unix: i64,
        refresh_before_seconds: i64,
        refresh: F,
    ) -> SdkResult<bool>
    where
        F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
    {
        let mut current = self.session.lock().map_err(|_| SdkError::InvalidSession)?;
        let Some(session) = current.as_mut() else {
            return Err(SdkError::InvalidSession);
        };

        if !session.needs_access_token_refresh(now_unix, refresh_before_seconds) {
            return Ok(false);
        }
        if session.is_refresh_token_expired(now_unix) {
            *current = None;
            return Err(SdkError::ExpiredRefreshToken);
        }

        let refresh_result =
            refresh(session).and_then(|next| session.apply_refresh(next, now_unix));
        if let Err(error) = refresh_result {
            *current = None;
            return Err(error);
        }

        Ok(true)
    }
}

impl ClientSessionState {
    pub fn from_auth_response(
        response: ClientAuthSessionResponse,
        now_unix: i64,
    ) -> SdkResult<Self> {
        Self::from_init(response.into_session_init(), now_unix)
    }

    pub fn from_init(init: SessionInit, now_unix: i64) -> SdkResult<Self> {
        validate_required("session_id", &init.session_id)?;
        validate_required("device_id", &init.device_id)?;
        validate_required("access_token", &init.access_token)?;
        validate_required("refresh_token", &init.refresh_token)?;
        validate_positive_ttl("expires_in", init.expires_in)?;
        validate_positive_ttl("refresh_expires_in", init.refresh_expires_in)?;

        Ok(Self {
            session_id: init.session_id,
            device_id: init.device_id,
            token_type: normalize_token_type(init.token_type)?,
            access_token: init.access_token,
            refresh_token: init.refresh_token,
            access_token_expires_at_unix: now_unix.saturating_add(init.expires_in),
            refresh_token_expires_at_unix: now_unix.saturating_add(init.refresh_expires_in),
            features: init.features,
        })
    }

    pub fn from_json(json: &str) -> SdkResult<Self> {
        let session: Self = serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)?;
        session.validate()?;

        Ok(session)
    }

    pub fn to_json(&self) -> SdkResult<String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| SdkError::InvalidSession)
    }

    pub fn validate(&self) -> SdkResult<()> {
        validate_required("session_id", &self.session_id)?;
        validate_required("device_id", &self.device_id)?;
        validate_required("token_type", &self.token_type)?;
        validate_required("access_token", &self.access_token)?;
        validate_required("refresh_token", &self.refresh_token)?;
        if self.access_token_expires_at_unix <= 0 || self.refresh_token_expires_at_unix <= 0 {
            return Err(SdkError::InvalidSession);
        }
        if self.refresh_token_expires_at_unix <= self.access_token_expires_at_unix {
            return Err(SdkError::InvalidSession);
        }

        Ok(())
    }

    pub fn authorization_header_value(&self) -> SdkResult<String> {
        self.validate()?;

        Ok(format!("{} {}", self.token_type, self.access_token))
    }

    pub fn is_access_token_expired(&self, now_unix: i64) -> bool {
        now_unix >= self.access_token_expires_at_unix
    }

    pub fn is_refresh_token_expired(&self, now_unix: i64) -> bool {
        now_unix >= self.refresh_token_expires_at_unix
    }

    pub fn needs_access_token_refresh(&self, now_unix: i64, refresh_before_seconds: i64) -> bool {
        let refresh_before_seconds = refresh_before_seconds.max(0);

        now_unix.saturating_add(refresh_before_seconds) >= self.access_token_expires_at_unix
    }

    pub fn apply_refresh(&mut self, refresh: SessionRefresh, now_unix: i64) -> SdkResult<()> {
        validate_required("access_token", &refresh.access_token)?;
        validate_required("refresh_token", &refresh.refresh_token)?;
        validate_positive_ttl("expires_in", refresh.expires_in)?;
        validate_positive_ttl("refresh_expires_in", refresh.refresh_expires_in)?;

        self.token_type = normalize_token_type(refresh.token_type)?;
        self.access_token = refresh.access_token;
        self.refresh_token = refresh.refresh_token;
        self.access_token_expires_at_unix = now_unix.saturating_add(refresh.expires_in);
        self.refresh_token_expires_at_unix = now_unix.saturating_add(refresh.refresh_expires_in);
        self.features = refresh.features;
        self.validate()
    }
}

fn normalize_token_type(token_type: Option<String>) -> SdkResult<String> {
    let token_type = token_type.unwrap_or_else(|| DEFAULT_TOKEN_TYPE.to_owned());
    let token_type = token_type.trim();
    if token_type.is_empty() || token_type.contains(char::is_whitespace) {
        return Err(SdkError::InvalidSession);
    }

    Ok(token_type.to_owned())
}

fn validate_required(_field: &'static str, value: &str) -> SdkResult<()> {
    if value.trim().is_empty() {
        return Err(SdkError::InvalidSession);
    }

    Ok(())
}

fn validate_positive_ttl(_field: &'static str, value: i64) -> SdkResult<()> {
    if value <= 0 {
        return Err(SdkError::InvalidSession);
    }

    Ok(())
}

fn validate_optional_device_key_id(device_key_id: Option<&str>) -> SdkResult<()> {
    let Some(device_key_id) = device_key_id else {
        return Ok(());
    };
    let device_key_id = device_key_id.trim();
    if device_key_id.is_empty() || device_key_id.contains(char::is_whitespace) {
        return Err(SdkError::InvalidSession);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Barrier,
        },
        thread,
        time::Duration,
    };

    use crate::error::SdkError;

    use super::{
        ClientAuthSessionResponse, ClientSessionState, SessionInit, SessionManager, SessionRefresh,
    };

    #[test]
    fn session_state_computes_expiry_and_authorization_header() {
        let session = fixture_session(100);

        assert_eq!(session.access_token_expires_at_unix, 1000);
        assert_eq!(session.refresh_token_expires_at_unix, 2600);
        assert_eq!(
            session
                .authorization_header_value()
                .expect("authorization header"),
            "Bearer access-token"
        );
    }

    #[test]
    fn session_state_detects_refresh_window() {
        let session = fixture_session(100);

        assert!(!session.needs_access_token_refresh(899, 60));
        assert!(session.needs_access_token_refresh(940, 60));
        assert!(session.is_access_token_expired(1000));
        assert!(!session.is_refresh_token_expired(2599));
    }

    #[test]
    fn session_state_applies_refresh_token_rotation() {
        let mut session = fixture_session(100);

        session
            .apply_refresh(
                SessionRefresh {
                    access_token: "next-access".to_owned(),
                    refresh_token: "next-refresh".to_owned(),
                    token_type: None,
                    expires_in: 900,
                    refresh_expires_in: 2_500,
                    features: serde_json::json!({ "tier": "pro" }),
                },
                200,
            )
            .expect("refresh should apply");

        assert_eq!(session.access_token, "next-access");
        assert_eq!(session.refresh_token, "next-refresh");
        assert_eq!(session.access_token_expires_at_unix, 1100);
        assert_eq!(session.refresh_token_expires_at_unix, 2700);
        assert_eq!(session.features["tier"], "pro");
    }

    #[test]
    fn session_state_rejects_blank_tokens_or_invalid_ttls() {
        let mut init = fixture_init();
        init.refresh_token = " ".to_owned();
        assert!(ClientSessionState::from_init(init, 100).is_err());

        let mut init = fixture_init();
        init.expires_in = 0;
        assert!(ClientSessionState::from_init(init, 100).is_err());
    }

    #[test]
    fn session_state_round_trips_json_and_validates_cached_shape() {
        let session = fixture_session(100);
        let json = session.to_json().expect("session should serialize");

        let decoded = ClientSessionState::from_json(&json).expect("session should deserialize");

        assert_eq!(decoded, session);
        assert!(ClientSessionState::from_json(r#"{"session_id":""}"#).is_err());
    }

    #[test]
    fn auth_session_response_builds_session_state() {
        let response = ClientAuthSessionResponse::from_json(
            r#"{
              "access_token": "access-token",
              "refresh_token": "refresh-token",
              "token_type": "Bearer",
              "expires_in": 900,
              "refresh_expires_in": 2500,
              "session_id": "session-id",
              "device_id": "device-id",
              "device_key_id": "ignored",
              "features": { "tier": "pro" }
            }"#,
        )
        .expect("auth response should parse");

        assert_eq!(response.device_key_id.as_deref(), Some("ignored"));
        let session =
            ClientSessionState::from_auth_response(response, 100).expect("session should build");

        assert_eq!(session.session_id, "session-id");
        assert_eq!(session.device_id, "device-id");
        assert_eq!(session.features["tier"], "pro");
        assert_eq!(session.access_token_expires_at_unix, 1000);
    }

    #[test]
    fn auth_session_response_parses_api_response_wrapper() {
        let response = ClientAuthSessionResponse::from_api_response_json(
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
              "request_id": "req_1"
            }"#,
        )
        .expect("api response should parse");

        assert_eq!(response.device_key_id.as_deref(), Some("device-key-id"));
        let session =
            ClientSessionState::from_auth_response(response, 100).expect("session should build");
        assert_eq!(session.device_id, "device-id");
    }

    #[test]
    fn auth_session_response_converts_to_refresh_payload() {
        let mut session = fixture_session(100);
        let response = ClientAuthSessionResponse::from_json(
            r#"{
              "access_token": "next-access",
              "refresh_token": "next-refresh",
              "token_type": "Bearer",
              "expires_in": 900,
              "refresh_expires_in": 2500,
              "session_id": "session-id",
              "device_id": "device-id",
              "features": { "tier": "pro" }
            }"#,
        )
        .expect("auth response should parse");

        session
            .apply_refresh(response.into_session_refresh(), 200)
            .expect("refresh should apply");

        assert_eq!(session.access_token, "next-access");
        assert_eq!(session.refresh_token, "next-refresh");
        assert_eq!(session.features["tier"], "pro");
    }

    #[test]
    fn auth_session_response_rejects_blank_device_key_id() {
        assert!(ClientAuthSessionResponse::from_json(
            r#"{
              "access_token": "access-token",
              "refresh_token": "refresh-token",
              "token_type": "Bearer",
              "expires_in": 900,
              "refresh_expires_in": 2500,
              "session_id": "session-id",
              "device_id": "device-id",
              "device_key_id": " ",
              "features": {}
            }"#,
        )
        .is_err());
    }

    #[test]
    fn session_manager_refreshes_only_once_for_concurrent_callers() {
        let manager = Arc::new(SessionManager::new(Some(fixture_session(100))));
        let refresh_count = Arc::new(AtomicUsize::new(0));
        let caller_count = 8;
        let barrier = Arc::new(Barrier::new(caller_count));
        let mut handles = Vec::new();

        for _ in 0..caller_count {
            let manager = Arc::clone(&manager);
            let refresh_count = Arc::clone(&refresh_count);
            let barrier = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                manager.authorization_header_value(940, 60, |session| {
                    refresh_count.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(session.refresh_token, "refresh-token");
                    thread::sleep(Duration::from_millis(10));
                    Ok(SessionRefresh {
                        access_token: "next-access".to_owned(),
                        refresh_token: "next-refresh".to_owned(),
                        token_type: None,
                        expires_in: 900,
                        refresh_expires_in: 2_500,
                        features: serde_json::json!({}),
                    })
                })
            }));
        }

        for handle in handles {
            let authorization = handle
                .join()
                .expect("thread should not panic")
                .expect("authorization should be available");
            assert_eq!(authorization, "Bearer next-access");
        }
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn session_manager_clears_session_after_refresh_failure() {
        let manager = SessionManager::new(Some(fixture_session(100)));

        let error = manager
            .refresh_if_needed(940, 60, |_| Err(SdkError::InvalidAccessToken))
            .expect_err("refresh failure should be returned");

        assert!(matches!(error, SdkError::InvalidAccessToken));
        assert!(manager
            .current_session()
            .expect("session lock should be readable")
            .is_none());
    }

    #[test]
    fn session_manager_clears_expired_refresh_token() {
        let manager = SessionManager::new(Some(fixture_session(100)));

        let error = manager
            .refresh_if_needed(2600, 60, |_| unreachable!("refresh should not run"))
            .expect_err("expired refresh token should fail");

        assert!(matches!(error, SdkError::ExpiredRefreshToken));
        assert!(manager
            .current_session()
            .expect("session lock should be readable")
            .is_none());
    }

    fn fixture_session(now_unix: i64) -> ClientSessionState {
        ClientSessionState::from_init(fixture_init(), now_unix).expect("session should build")
    }

    fn fixture_init() -> SessionInit {
        SessionInit {
            session_id: "session-id".to_owned(),
            device_id: "device-id".to_owned(),
            access_token: "access-token".to_owned(),
            refresh_token: "refresh-token".to_owned(),
            token_type: None,
            expires_in: 900,
            refresh_expires_in: 2_500,
            features: serde_json::json!({}),
        }
    }
}
