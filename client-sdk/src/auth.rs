use serde::{Deserialize, Serialize};

use crate::{
    device::DeviceIdentity,
    session::{ClientAuthSessionResponse, ClientSessionState, SessionManager},
    SdkError, SdkResult,
};

#[derive(Debug, Clone)]
pub struct ActivationRequestInput<'a> {
    pub app_key: &'a str,
    pub license_key: &'a str,
    pub device: &'a DeviceIdentity,
    pub device_name: Option<&'a str>,
    pub os: Option<&'a str>,
    pub app_version: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct CustomerLoginRequestInput<'a> {
    pub app_key: &'a str,
    pub email: &'a str,
    pub password: &'a str,
    pub device: &'a DeviceIdentity,
    pub device_name: Option<&'a str>,
    pub os: Option<&'a str>,
    pub app_version: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActivationRequestPayload {
    pub app_key: String,
    pub license_key: String,
    pub machine_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    pub device_public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CustomerLoginRequestPayload {
    pub app_key: String,
    pub email: String,
    pub password: String,
    pub machine_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
    pub device_public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RefreshRequestPayload {
    pub refresh_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeartbeatRequestPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EmailVerifyConfirmRequestPayload {
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PasswordResetConfirmRequestPayload {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HeartbeatResponse {
    pub status: String,
    pub server_time: i64,
    pub license_status: String,
    #[serde(default)]
    pub entitlement_id: Option<String>,
    #[serde(default)]
    pub entitlement_kind: Option<String>,
    #[serde(default)]
    pub entitlement_status: Option<String>,
    #[serde(default)]
    pub entitlement_active: bool,
    #[serde(default)]
    pub subscription_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VerifyResponse {
    pub valid: bool,
    pub features: serde_json::Value,
    pub expires_at: Option<String>,
    #[serde(default)]
    pub entitlement_id: Option<String>,
    #[serde(default)]
    pub entitlement_kind: Option<String>,
    #[serde(default)]
    pub entitlement_status: Option<String>,
    #[serde(default)]
    pub entitlement_active: bool,
    #[serde(default)]
    pub subscription_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LogoutResponse {
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EmailVerifyRequestResponse {
    pub expires_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EmailVerifyConfirmResponse {
    pub customer_id: String,
    pub email_verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PasswordResetConfirmResponse {
    pub ok: bool,
    pub revoked_sessions: u64,
    pub revoked_refresh_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ClientBootstrap {
    pub device: DeviceIdentity,
    pub session_manager: SessionManager,
}

impl ClientBootstrap {
    pub fn new(device: DeviceIdentity) -> SdkResult<Self> {
        validate_device_identity(&device)?;

        Ok(Self {
            device,
            session_manager: SessionManager::new(None),
        })
    }

    pub fn activation_request(
        &self,
        app_key: &str,
        license_key: &str,
        device_name: Option<&str>,
        os: Option<&str>,
        app_version: Option<&str>,
    ) -> SdkResult<ActivationRequestPayload> {
        build_activation_request(ActivationRequestInput {
            app_key,
            license_key,
            device: &self.device,
            device_name,
            os,
            app_version,
        })
    }

    pub fn customer_login_request(
        &self,
        app_key: &str,
        email: &str,
        password: &str,
        device_name: Option<&str>,
        os: Option<&str>,
        app_version: Option<&str>,
    ) -> SdkResult<CustomerLoginRequestPayload> {
        build_customer_login_request(CustomerLoginRequestInput {
            app_key,
            email,
            password,
            device: &self.device,
            device_name,
            os,
            app_version,
        })
    }

    pub fn apply_auth_response(
        &self,
        response: ClientAuthSessionResponse,
        now_unix: i64,
    ) -> SdkResult<ClientSessionState> {
        let session = ClientSessionState::from_auth_response(response, now_unix)?;
        self.session_manager.set_session(session.clone())?;

        Ok(session)
    }

    pub fn apply_auth_response_json(
        &self,
        json: &str,
        now_unix: i64,
    ) -> SdkResult<ClientSessionState> {
        self.apply_auth_response(ClientAuthSessionResponse::from_json(json)?, now_unix)
    }

    pub fn clear_session(&self) -> SdkResult<()> {
        self.session_manager.clear_session()
    }
}

impl HeartbeatResponse {
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

    fn validate(&self) -> SdkResult<()> {
        if self.status.trim().is_empty()
            || self.server_time <= 0
            || self.license_status.trim().is_empty()
        {
            return Err(SdkError::InvalidSession);
        }

        Ok(())
    }
}

impl VerifyResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        crate::response::parse_api_response_data(json).map(|response| response.data)
    }
}

impl LogoutResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        crate::response::parse_api_response_data(json).map(|response| response.data)
    }
}

impl EmailVerifyRequestResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        crate::response::parse_api_response_data(json).map(|response| response.data)
    }
}

impl EmailVerifyConfirmResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        crate::response::parse_api_response_data(json).map(|response| response.data)
    }
}

impl PasswordResetConfirmResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        serde_json::from_str(json).map_err(|_| SdkError::InvalidSession)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        crate::response::parse_api_response_data(json).map(|response| response.data)
    }
}

pub fn build_activation_request(
    input: ActivationRequestInput<'_>,
) -> SdkResult<ActivationRequestPayload> {
    validate_device_identity(input.device)?;

    Ok(ActivationRequestPayload {
        app_key: clean_required("app_key", input.app_key)?,
        license_key: clean_required("license_key", input.license_key)?,
        machine_id: input.device.machine_id.clone(),
        device_name: clean_optional(input.device_name),
        os: clean_optional(input.os),
        app_version: clean_optional(input.app_version),
        device_public_key: input.device.device_public_key.clone(),
    })
}

pub fn build_customer_login_request(
    input: CustomerLoginRequestInput<'_>,
) -> SdkResult<CustomerLoginRequestPayload> {
    validate_device_identity(input.device)?;

    Ok(CustomerLoginRequestPayload {
        app_key: clean_required("app_key", input.app_key)?,
        email: clean_required("email", input.email)?,
        password: clean_required("password", input.password)?,
        machine_id: input.device.machine_id.clone(),
        device_name: clean_optional(input.device_name),
        os: clean_optional(input.os),
        app_version: clean_optional(input.app_version),
        device_public_key: input.device.device_public_key.clone(),
    })
}

pub fn build_refresh_request(refresh_token: &str) -> SdkResult<RefreshRequestPayload> {
    Ok(RefreshRequestPayload {
        refresh_token: clean_required("refresh_token", refresh_token)?,
    })
}

pub fn build_heartbeat_request(app_version: Option<&str>) -> HeartbeatRequestPayload {
    HeartbeatRequestPayload {
        app_version: clean_optional(app_version),
    }
}

pub fn build_email_verify_confirm_request(
    token: &str,
) -> SdkResult<EmailVerifyConfirmRequestPayload> {
    Ok(EmailVerifyConfirmRequestPayload {
        token: clean_required("token", token)?,
    })
}

pub fn build_password_reset_confirm_request(
    token: &str,
    new_password: &str,
) -> SdkResult<PasswordResetConfirmRequestPayload> {
    Ok(PasswordResetConfirmRequestPayload {
        token: clean_required("token", token)?,
        new_password: clean_required("new_password", new_password)?,
    })
}

fn validate_device_identity(device: &DeviceIdentity) -> SdkResult<()> {
    if device.machine_id.trim().is_empty() {
        return Err(SdkError::InvalidMachineId);
    }
    if device.device_public_key.trim().is_empty() {
        return Err(SdkError::InvalidPublicKey);
    }
    device.private_key_pkcs8_der()?;

    Ok(())
}

fn clean_required(field: &'static str, value: &str) -> SdkResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(SdkError::InvalidAuthRequest(field));
    }

    Ok(value.to_owned())
}

fn clean_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use crate::device::DeviceIdentity;

    use super::{
        build_activation_request, build_customer_login_request, build_email_verify_confirm_request,
        build_heartbeat_request, build_password_reset_confirm_request, build_refresh_request,
        ActivationRequestInput, ClientBootstrap, CustomerLoginRequestInput,
        EmailVerifyConfirmResponse, EmailVerifyRequestResponse, HeartbeatResponse, LogoutResponse,
        PasswordResetConfirmResponse, VerifyResponse,
    };

    #[test]
    fn activation_request_uses_device_identity_and_trims_fields() {
        let device = fixture_device();

        let payload = build_activation_request(ActivationRequestInput {
            app_key: " app_key ",
            license_key: " license ",
            device: &device,
            device_name: Some(" Workstation "),
            os: Some(" Windows "),
            app_version: Some(" 1.0.0 "),
        })
        .expect("activation request should build");

        assert_eq!(payload.app_key, "app_key");
        assert_eq!(payload.license_key, "license");
        assert_eq!(payload.machine_id, device.machine_id);
        assert_eq!(payload.device_public_key, device.device_public_key);
        assert_eq!(payload.device_name.as_deref(), Some("Workstation"));
        assert_eq!(payload.os.as_deref(), Some("Windows"));
        assert_eq!(payload.app_version.as_deref(), Some("1.0.0"));
    }

    #[test]
    fn customer_login_request_uses_device_identity_and_skips_blank_optional_fields() {
        let device = fixture_device();

        let payload = build_customer_login_request(CustomerLoginRequestInput {
            app_key: "app_key",
            email: " user@example.com ",
            password: " Password@123 ",
            device: &device,
            device_name: Some(" "),
            os: None,
            app_version: Some(" 1.0.0 "),
        })
        .expect("login request should build");
        let json = serde_json::to_string(&payload).expect("payload should serialize");

        assert_eq!(payload.email, "user@example.com");
        assert_eq!(payload.password, "Password@123");
        assert_eq!(payload.device_name, None);
        assert!(json.contains("\"machine_id\""));
        assert!(!json.contains("device_name"));
    }

    #[test]
    fn auth_request_rejects_blank_required_fields() {
        let device = fixture_device();

        assert!(build_activation_request(ActivationRequestInput {
            app_key: " ",
            license_key: "license",
            device: &device,
            device_name: None,
            os: None,
            app_version: None,
        })
        .is_err());
        assert!(build_customer_login_request(CustomerLoginRequestInput {
            app_key: "app",
            email: " ",
            password: "password",
            device: &device,
            device_name: None,
            os: None,
            app_version: None,
        })
        .is_err());
        assert!(build_refresh_request(" ").is_err());
        assert!(build_email_verify_confirm_request(" ").is_err());
        assert!(build_password_reset_confirm_request("token", " ").is_err());
    }

    #[test]
    fn auxiliary_auth_payloads_trim_or_skip_fields() {
        let refresh = build_refresh_request(" refresh-token ").expect("refresh payload");
        let heartbeat = build_heartbeat_request(Some(" 1.2.3 "));
        let verify = build_email_verify_confirm_request(" token ").expect("verify payload");
        let reset = build_password_reset_confirm_request(" reset-token ", " NewPassword@123 ")
            .expect("reset payload");

        assert_eq!(refresh.refresh_token, "refresh-token");
        assert_eq!(heartbeat.app_version.as_deref(), Some("1.2.3"));
        assert_eq!(build_heartbeat_request(Some(" ")).app_version, None);
        assert_eq!(verify.token, "token");
        assert_eq!(reset.token, "reset-token");
        assert_eq!(reset.new_password, "NewPassword@123");
    }

    #[test]
    fn client_bootstrap_builds_requests_and_stores_auth_session() {
        let bootstrap =
            ClientBootstrap::new(fixture_device()).expect("bootstrap should initialize");
        let activation = bootstrap
            .activation_request(
                "app_key",
                "license",
                Some("Workstation"),
                Some("Windows"),
                Some("1.0.0"),
            )
            .expect("activation request should build");
        let login = bootstrap
            .customer_login_request(
                "app_key",
                "user@example.com",
                "Password@123",
                None,
                None,
                None,
            )
            .expect("login request should build");

        assert_eq!(activation.machine_id, bootstrap.device.machine_id);
        assert_eq!(login.device_public_key, bootstrap.device.device_public_key);

        let session = bootstrap
            .apply_auth_response_json(
                r#"{
                  "access_token": "access-token",
                  "refresh_token": "refresh-token",
                  "token_type": "Bearer",
                  "expires_in": 900,
                  "refresh_expires_in": 2500,
                  "session_id": "session-id",
                  "device_id": "device-id",
                  "features": {}
                }"#,
                100,
            )
            .expect("auth response should initialize session");
        let stored = bootstrap
            .session_manager
            .current_session()
            .expect("session manager should read")
            .expect("session should be stored");

        assert_eq!(session, stored);
        assert_eq!(stored.access_token, "access-token");

        bootstrap
            .clear_session()
            .expect("session should clear after logout");
        assert!(bootstrap
            .session_manager
            .current_session()
            .expect("session manager should read")
            .is_none());
    }

    #[test]
    fn heartbeat_response_parses_api_response_wrapper() {
        let response = HeartbeatResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "status": "ok",
                "server_time": 1710000000,
                "license_status": "active"
              },
              "request_id": "req_1"
            }"#,
        )
        .expect("heartbeat response should parse");

        assert_eq!(response.status, "ok");
        assert!(HeartbeatResponse::from_json(
            r#"{
              "status": "",
              "server_time": 0,
              "license_status": "active"
            }"#,
        )
        .is_err());
    }

    #[test]
    fn verify_response_parses_api_response_wrapper() {
        let response = VerifyResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "valid": true,
                "features": { "tier": "pro" },
                "expires_at": "2027-01-01T00:00:00Z"
              },
              "request_id": "req_1"
            }"#,
        )
        .expect("verify response should parse");

        assert!(response.valid);
        assert_eq!(response.features["tier"], "pro");
    }

    #[test]
    fn logout_response_parses_api_response_wrapper() {
        let response = LogoutResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": { "revoked": true },
              "request_id": "req_1"
            }"#,
        )
        .expect("logout response should parse");

        assert!(response.revoked);
    }

    #[test]
    fn email_and_password_responses_parse_api_response_wrapper() {
        let requested = EmailVerifyRequestResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": { "expires_at": "2027-01-01T00:00:00Z" },
              "request_id": "req_1"
            }"#,
        )
        .expect("request response should parse");
        assert_eq!(requested.expires_at, "2027-01-01T00:00:00Z");

        let confirmed = EmailVerifyConfirmResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "customer_id": "00000000-0000-0000-0000-000000000001",
                "email_verified": true
              },
              "request_id": "req_1"
            }"#,
        )
        .expect("confirm response should parse");
        assert!(confirmed.email_verified);

        let reset = PasswordResetConfirmResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "ok": true,
                "revoked_sessions": 2,
                "revoked_refresh_tokens": 3
              },
              "request_id": "req_1"
            }"#,
        )
        .expect("reset response should parse");
        assert!(reset.ok);
        assert_eq!(reset.revoked_sessions, 2);
        assert_eq!(reset.revoked_refresh_tokens, 3);
    }

    fn fixture_device() -> DeviceIdentity {
        DeviceIdentity::generate("app_key", &["machine"]).expect("device identity should generate")
    }
}
