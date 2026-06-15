use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::{error::Error, fmt};

#[derive(Debug)]
pub enum AppError {
    AccountDisabled(String),
    AlreadyRevoked(String),
    AppDisabled(String),
    AppNotFound(String),
    BusinessRuleFailed(String),
    Config(String),
    Conflict(String),
    Crypto(String),
    CsrfFailed(String),
    Dependency(String),
    DeviceBlacklisted(String),
    DeviceLimitExceeded(String),
    DeviceNotFound(String),
    DeviceNotActivated(String),
    DuplicateEmail(String),
    Forbidden(String),
    InviteTokenInvalid(String),
    InvalidLicenseState(String),
    InvalidReleaseState(String),
    InvalidRequest(String),
    InvalidScriptState(String),
    InvalidCredentials(String),
    LicenseExpired(String),
    LicenseInvalid(String),
    LicenseNotFound(String),
    RateLimited(String),
    LoginRateLimited(String),
    PasswordResetRateLimited(String),
    ActivationRateLimited(String),
    RefreshReuseDetected(String),
    RefreshRateLimited(String),
    ReleaseNotFound(String),
    MfaAlreadyEnabled(String),
    MfaFailed(String),
    MfaNotEnabled(String),
    MfaRequired(String),
    NotFound(String),
    NotificationChannelNameExists(String),
    PasswordResetTokenInvalid(String),
    SessionExpired(String),
    SignatureInvalid(String),
    SignatureRequired(String),
    ScriptNotFound(String),
    SubscriptionInactive(String),
    TenantForbidden(String),
    TenantNotFound(String),
    TokenExpired(String),
    TokenInvalid(String),
    Unauthenticated(String),
    UserNotFound(String),
    ValidationFailed(String),
    WeakPassword(String),
}

impl AppError {
    pub fn account_disabled() -> Self {
        Self::AccountDisabled("account disabled".to_owned())
    }

    pub fn already_revoked(message: impl Into<String>) -> Self {
        Self::AlreadyRevoked(message.into())
    }

    pub fn app_disabled() -> Self {
        Self::AppDisabled("application disabled".to_owned())
    }

    pub fn app_not_found() -> Self {
        Self::AppNotFound("application not found".to_owned())
    }

    pub fn business_rule_failed(message: impl Into<String>) -> Self {
        Self::BusinessRuleFailed(message.into())
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn dependency(message: impl Into<String>) -> Self {
        Self::Dependency(message.into())
    }

    pub fn device_blacklisted() -> Self {
        Self::DeviceBlacklisted("device blacklisted".to_owned())
    }

    pub fn device_limit_exceeded() -> Self {
        Self::DeviceLimitExceeded("device limit exceeded".to_owned())
    }

    pub fn device_not_found() -> Self {
        Self::DeviceNotFound("device not found".to_owned())
    }

    pub fn device_not_activated() -> Self {
        Self::DeviceNotActivated("device not activated".to_owned())
    }

    pub fn duplicate_email() -> Self {
        Self::DuplicateEmail("duplicate email".to_owned())
    }

    pub fn crypto(message: impl Into<String>) -> Self {
        Self::Crypto(message.into())
    }

    pub fn csrf_failed() -> Self {
        Self::CsrfFailed("csrf failed".to_owned())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest(message.into())
    }

    pub fn invalid_credentials() -> Self {
        Self::InvalidCredentials("invalid credentials".to_owned())
    }

    pub fn invalid_license_state(message: impl Into<String>) -> Self {
        Self::InvalidLicenseState(message.into())
    }

    pub fn invalid_release_state(message: impl Into<String>) -> Self {
        Self::InvalidReleaseState(message.into())
    }

    pub fn invalid_script_state(message: impl Into<String>) -> Self {
        Self::InvalidScriptState(message.into())
    }

    pub fn license_expired() -> Self {
        Self::LicenseExpired("license expired".to_owned())
    }

    pub fn license_invalid(message: impl Into<String>) -> Self {
        Self::LicenseInvalid(message.into())
    }

    pub fn license_not_found() -> Self {
        Self::LicenseNotFound("license not found".to_owned())
    }

    pub fn rate_limited() -> Self {
        Self::RateLimited("rate limited".to_owned())
    }

    pub fn login_rate_limited() -> Self {
        Self::LoginRateLimited("login rate limited".to_owned())
    }

    pub fn password_reset_rate_limited() -> Self {
        Self::PasswordResetRateLimited("password reset rate limited".to_owned())
    }

    pub fn activation_rate_limited() -> Self {
        Self::ActivationRateLimited("activation rate limited".to_owned())
    }

    pub fn refresh_rate_limited() -> Self {
        Self::RefreshRateLimited("refresh rate limited".to_owned())
    }

    pub fn refresh_reuse_detected() -> Self {
        Self::RefreshReuseDetected("refresh token reuse detected".to_owned())
    }

    pub fn release_not_found() -> Self {
        Self::ReleaseNotFound("release not found".to_owned())
    }

    pub fn mfa_required() -> Self {
        Self::MfaRequired("mfa required".to_owned())
    }

    pub fn mfa_failed() -> Self {
        Self::MfaFailed("mfa failed".to_owned())
    }

    pub fn mfa_already_enabled() -> Self {
        Self::MfaAlreadyEnabled("mfa already enabled".to_owned())
    }

    pub fn mfa_not_enabled() -> Self {
        Self::MfaNotEnabled("mfa not enabled".to_owned())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn notification_channel_name_exists() -> Self {
        Self::NotificationChannelNameExists("notification channel name exists".to_owned())
    }

    pub fn password_reset_token_invalid() -> Self {
        Self::PasswordResetTokenInvalid("password reset token invalid".to_owned())
    }

    pub fn session_expired() -> Self {
        Self::SessionExpired("session expired".to_owned())
    }

    pub fn signature_invalid(message: impl Into<String>) -> Self {
        Self::SignatureInvalid(message.into())
    }

    pub fn signature_required(message: impl Into<String>) -> Self {
        Self::SignatureRequired(message.into())
    }

    pub fn script_not_found() -> Self {
        Self::ScriptNotFound("secure script not found".to_owned())
    }

    pub fn subscription_inactive(message: impl Into<String>) -> Self {
        Self::SubscriptionInactive(message.into())
    }

    pub fn tenant_forbidden() -> Self {
        Self::TenantForbidden("tenant forbidden".to_owned())
    }

    pub fn tenant_not_found() -> Self {
        Self::TenantNotFound("tenant not found".to_owned())
    }

    pub fn token_expired() -> Self {
        Self::TokenExpired("token expired".to_owned())
    }

    pub fn token_invalid(message: impl Into<String>) -> Self {
        Self::TokenInvalid(message.into())
    }

    pub fn unauthenticated() -> Self {
        Self::Unauthenticated("unauthenticated".to_owned())
    }

    pub fn user_not_found() -> Self {
        Self::UserNotFound("user not found".to_owned())
    }

    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self::ValidationFailed(message.into())
    }

    pub fn weak_password() -> Self {
        Self::WeakPassword("password is too weak".to_owned())
    }

    pub fn invite_token_invalid() -> Self {
        Self::InviteTokenInvalid("invite token invalid".to_owned())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AccountDisabled(message)
            | Self::AlreadyRevoked(message)
            | Self::AppDisabled(message)
            | Self::AppNotFound(message)
            | Self::BusinessRuleFailed(message)
            | Self::Config(message)
            | Self::Conflict(message)
            | Self::Crypto(message)
            | Self::CsrfFailed(message)
            | Self::Dependency(message)
            | Self::DeviceBlacklisted(message)
            | Self::DeviceLimitExceeded(message)
            | Self::DeviceNotFound(message)
            | Self::DeviceNotActivated(message)
            | Self::DuplicateEmail(message)
            | Self::Forbidden(message)
            | Self::InvalidLicenseState(message)
            | Self::InvalidReleaseState(message)
            | Self::InvalidRequest(message)
            | Self::InvalidScriptState(message)
            | Self::InvalidCredentials(message)
            | Self::LicenseExpired(message)
            | Self::LicenseInvalid(message)
            | Self::LicenseNotFound(message)
            | Self::RateLimited(message)
            | Self::LoginRateLimited(message)
            | Self::PasswordResetRateLimited(message)
            | Self::ActivationRateLimited(message)
            | Self::RefreshReuseDetected(message)
            | Self::RefreshRateLimited(message)
            | Self::ReleaseNotFound(message)
            | Self::MfaAlreadyEnabled(message)
            | Self::MfaFailed(message)
            | Self::MfaNotEnabled(message)
            | Self::MfaRequired(message)
            | Self::NotFound(message)
            | Self::NotificationChannelNameExists(message)
            | Self::PasswordResetTokenInvalid(message)
            | Self::SessionExpired(message)
            | Self::SignatureInvalid(message)
            | Self::SignatureRequired(message)
            | Self::ScriptNotFound(message)
            | Self::SubscriptionInactive(message)
            | Self::TenantForbidden(message)
            | Self::TenantNotFound(message)
            | Self::TokenExpired(message)
            | Self::TokenInvalid(message)
            | Self::Unauthenticated(message)
            | Self::UserNotFound(message)
            | Self::ValidationFailed(message)
            | Self::WeakPassword(message)
            | Self::InviteTokenInvalid(message) => write!(f, "{message}"),
        }
    }
}

impl Error for AppError {}

#[derive(Serialize)]
pub struct ApiResponse<T>
where
    T: Serialize,
{
    pub code: u32,
    pub message: &'static str,
    pub data: T,
    pub request_id: String,
}

impl<T> ApiResponse<T>
where
    T: Serialize,
{
    pub fn ok(data: T, request_id: impl Into<String>) -> Self {
        Self {
            code: 0,
            message: "ok",
            data,
            request_id: request_id.into(),
        }
    }
}

#[derive(Serialize)]
struct ApiErrorResponse {
    code: u32,
    #[serde(rename = "errorCode")]
    error_code: &'static str,
    message: &'static str,
    data: Option<()>,
    request_id: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::AccountDisabled(_) => (StatusCode::FORBIDDEN, 40302, "account_disabled"),
            Self::AlreadyRevoked(_) => (StatusCode::CONFLICT, 40906, "already_revoked"),
            Self::AppDisabled(_) => (StatusCode::FORBIDDEN, 40310, "app_disabled"),
            Self::AppNotFound(_) => (StatusCode::NOT_FOUND, 40403, "app_not_found"),
            Self::BusinessRuleFailed(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                42200,
                "business_rule_failed",
            ),
            Self::Config(_) => (StatusCode::INTERNAL_SERVER_ERROR, 50000, "internal_error"),
            Self::Conflict(_) => (StatusCode::CONFLICT, 40900, "conflict"),
            Self::Crypto(_) => (StatusCode::INTERNAL_SERVER_ERROR, 50005, "crypto_error"),
            Self::CsrfFailed(_) => (StatusCode::FORBIDDEN, 40105, "csrf_failed"),
            Self::Dependency(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                50300,
                "service_unavailable",
            ),
            Self::DeviceBlacklisted(_) => (StatusCode::FORBIDDEN, 40303, "device_blacklisted"),
            Self::DeviceLimitExceeded(_) => (StatusCode::CONFLICT, 40904, "device_limit_exceeded"),
            Self::DeviceNotFound(_) => (StatusCode::NOT_FOUND, 40405, "device_not_found"),
            Self::DeviceNotActivated(_) => (StatusCode::FORBIDDEN, 40309, "device_not_activated"),
            Self::DuplicateEmail(_) => (StatusCode::CONFLICT, 40901, "duplicate_email"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, 40300, "forbidden"),
            Self::InvalidLicenseState(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                42202,
                "invalid_license_state",
            ),
            Self::InvalidReleaseState(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                42203,
                "invalid_release_state",
            ),
            Self::InvalidRequest(_) => (StatusCode::BAD_REQUEST, 40000, "invalid_request"),
            Self::InvalidScriptState(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                42204,
                "invalid_script_state",
            ),
            Self::InvalidCredentials(_) => (StatusCode::UNAUTHORIZED, 40102, "invalid_credentials"),
            Self::LicenseExpired(_) => (StatusCode::FORBIDDEN, 40305, "license_expired"),
            Self::LicenseInvalid(_) => (StatusCode::FORBIDDEN, 40304, "license_invalid"),
            Self::LicenseNotFound(_) => (StatusCode::NOT_FOUND, 40404, "license_not_found"),
            Self::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, 42900, "rate_limited"),
            Self::LoginRateLimited(_) => {
                (StatusCode::TOO_MANY_REQUESTS, 42901, "login_rate_limited")
            }
            Self::PasswordResetRateLimited(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                42902,
                "password_reset_rate_limited",
            ),
            Self::ActivationRateLimited(_) => (
                StatusCode::TOO_MANY_REQUESTS,
                42903,
                "activation_rate_limited",
            ),
            Self::RefreshReuseDetected(_) => {
                (StatusCode::UNAUTHORIZED, 40108, "refresh_reuse_detected")
            }
            Self::RefreshRateLimited(_) => {
                (StatusCode::TOO_MANY_REQUESTS, 42904, "refresh_rate_limited")
            }
            Self::ReleaseNotFound(_) => (StatusCode::NOT_FOUND, 40406, "release_not_found"),
            Self::MfaAlreadyEnabled(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                42206,
                "mfa_already_enabled",
            ),
            Self::MfaFailed(_) => (StatusCode::UNAUTHORIZED, 40104, "mfa_failed"),
            Self::MfaNotEnabled(_) => (StatusCode::UNPROCESSABLE_ENTITY, 42207, "mfa_not_enabled"),
            Self::MfaRequired(_) => (StatusCode::UNAUTHORIZED, 40103, "mfa_required"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, 40400, "not_found"),
            Self::NotificationChannelNameExists(_) => (
                StatusCode::CONFLICT,
                40909,
                "notification_channel_name_exists",
            ),
            Self::PasswordResetTokenInvalid(_) => (
                StatusCode::UNAUTHORIZED,
                40109,
                "password_reset_token_invalid",
            ),
            Self::SessionExpired(_) => (StatusCode::UNAUTHORIZED, 40101, "session_expired"),
            Self::SignatureInvalid(_) => (StatusCode::FORBIDDEN, 40308, "signature_invalid"),
            Self::SignatureRequired(_) => (StatusCode::FORBIDDEN, 40307, "signature_required"),
            Self::ScriptNotFound(_) => (StatusCode::NOT_FOUND, 40407, "script_not_found"),
            Self::SubscriptionInactive(_) => {
                (StatusCode::FORBIDDEN, 40306, "subscription_inactive")
            }
            Self::TenantForbidden(_) => (StatusCode::FORBIDDEN, 40301, "tenant_forbidden"),
            Self::TenantNotFound(_) => (StatusCode::NOT_FOUND, 40401, "tenant_not_found"),
            Self::TokenExpired(_) => (StatusCode::UNAUTHORIZED, 40106, "token_expired"),
            Self::TokenInvalid(_) => (StatusCode::UNAUTHORIZED, 40107, "token_invalid"),
            Self::Unauthenticated(_) => (StatusCode::UNAUTHORIZED, 40100, "unauthenticated"),
            Self::UserNotFound(_) => (StatusCode::NOT_FOUND, 40402, "user_not_found"),
            Self::ValidationFailed(_) => (StatusCode::BAD_REQUEST, 40001, "validation_failed"),
            Self::WeakPassword(_) => (StatusCode::UNPROCESSABLE_ENTITY, 42201, "weak_password"),
            Self::InviteTokenInvalid(_) => {
                (StatusCode::UNAUTHORIZED, 40110, "invite_token_invalid")
            }
        };

        let error_code = detailed_error_code(&self).unwrap_or(message);
        let body = ApiErrorResponse {
            code,
            error_code,
            message,
            data: None,
            request_id: "req_bootstrap".to_owned(),
        };

        (status, Json(body)).into_response()
    }
}

fn detailed_error_code(error: &AppError) -> Option<&'static str> {
    let message = match error {
        AppError::BusinessRuleFailed(message)
        | AppError::Conflict(message)
        | AppError::Forbidden(message)
        | AppError::InvalidRequest(message)
        | AppError::NotFound(message)
        | AppError::SubscriptionInactive(message)
        | AppError::ValidationFailed(message) => message.as_str(),
        _ => return None,
    };
    message
        .split_once(':')
        .map(|(code, _)| code)
        .filter(|code| is_stable_error_code(code))
        .and_then(stable_error_code)
}

fn is_stable_error_code(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn stable_error_code(value: &str) -> Option<&'static str> {
    match value {
        "asset_not_ready" => Some("asset_not_ready"),
        "model_not_support_reference_video" => Some("model_not_support_reference_video"),
        "model_not_support_reference_audio" => Some("model_not_support_reference_audio"),
        "model_not_support_first_frame" => Some("model_not_support_first_frame"),
        "model_not_support_last_frame" => Some("model_not_support_last_frame"),
        "reference_asset_conflict" => Some("reference_asset_conflict"),
        "reference_asset_kind_mismatch" => Some("reference_asset_kind_mismatch"),
        "reference_asset_kind_not_allowed" => Some("reference_asset_kind_not_allowed"),
        "reference_asset_mime_not_allowed" => Some("reference_asset_mime_not_allowed"),
        "reference_asset_role_invalid" => Some("reference_asset_role_invalid"),
        "reference_asset_too_large" => Some("reference_asset_too_large"),
        "reference_image_too_many" => Some("reference_image_too_many"),
        "reference_asset_too_many" => Some("reference_asset_too_many"),
        "reference_video_too_many" => Some("reference_video_too_many"),
        "reference_audio_too_many" => Some("reference_audio_too_many"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use axum::{body::to_bytes, http::StatusCode, response::IntoResponse};
    use serde_json::Value;

    use super::AppError;

    #[tokio::test]
    async fn app_error_response_codes_match_documented_error_codes() {
        let documented_errors = include_str!("../../权限点与错误码清单.md");
        let cases = [
            (
                AppError::account_disabled(),
                StatusCode::FORBIDDEN,
                40302,
                "account_disabled",
            ),
            (
                AppError::already_revoked("resource already revoked"),
                StatusCode::CONFLICT,
                40906,
                "already_revoked",
            ),
            (
                AppError::app_disabled(),
                StatusCode::FORBIDDEN,
                40310,
                "app_disabled",
            ),
            (
                AppError::app_not_found(),
                StatusCode::NOT_FOUND,
                40403,
                "app_not_found",
            ),
            (
                AppError::business_rule_failed("business rule failed"),
                StatusCode::UNPROCESSABLE_ENTITY,
                42200,
                "business_rule_failed",
            ),
            (
                AppError::config("config error"),
                StatusCode::INTERNAL_SERVER_ERROR,
                50000,
                "internal_error",
            ),
            (
                AppError::conflict("conflict"),
                StatusCode::CONFLICT,
                40900,
                "conflict",
            ),
            (
                AppError::crypto("crypto error"),
                StatusCode::INTERNAL_SERVER_ERROR,
                50005,
                "crypto_error",
            ),
            (
                AppError::csrf_failed(),
                StatusCode::FORBIDDEN,
                40105,
                "csrf_failed",
            ),
            (
                AppError::dependency("dependency error"),
                StatusCode::SERVICE_UNAVAILABLE,
                50300,
                "service_unavailable",
            ),
            (
                AppError::device_blacklisted(),
                StatusCode::FORBIDDEN,
                40303,
                "device_blacklisted",
            ),
            (
                AppError::device_limit_exceeded(),
                StatusCode::CONFLICT,
                40904,
                "device_limit_exceeded",
            ),
            (
                AppError::device_not_found(),
                StatusCode::NOT_FOUND,
                40405,
                "device_not_found",
            ),
            (
                AppError::device_not_activated(),
                StatusCode::FORBIDDEN,
                40309,
                "device_not_activated",
            ),
            (
                AppError::duplicate_email(),
                StatusCode::CONFLICT,
                40901,
                "duplicate_email",
            ),
            (
                AppError::notification_channel_name_exists(),
                StatusCode::CONFLICT,
                40909,
                "notification_channel_name_exists",
            ),
            (
                AppError::forbidden("forbidden"),
                StatusCode::FORBIDDEN,
                40300,
                "forbidden",
            ),
            (
                AppError::invalid_credentials(),
                StatusCode::UNAUTHORIZED,
                40102,
                "invalid_credentials",
            ),
            (
                AppError::invalid_license_state("license state is invalid"),
                StatusCode::UNPROCESSABLE_ENTITY,
                42202,
                "invalid_license_state",
            ),
            (
                AppError::invalid_release_state("release state is invalid"),
                StatusCode::UNPROCESSABLE_ENTITY,
                42203,
                "invalid_release_state",
            ),
            (
                AppError::invalid_request("invalid request"),
                StatusCode::BAD_REQUEST,
                40000,
                "invalid_request",
            ),
            (
                AppError::invalid_script_state("script state is invalid"),
                StatusCode::UNPROCESSABLE_ENTITY,
                42204,
                "invalid_script_state",
            ),
            (
                AppError::invite_token_invalid(),
                StatusCode::UNAUTHORIZED,
                40110,
                "invite_token_invalid",
            ),
            (
                AppError::license_expired(),
                StatusCode::FORBIDDEN,
                40305,
                "license_expired",
            ),
            (
                AppError::license_invalid("license invalid"),
                StatusCode::FORBIDDEN,
                40304,
                "license_invalid",
            ),
            (
                AppError::license_not_found(),
                StatusCode::NOT_FOUND,
                40404,
                "license_not_found",
            ),
            (
                AppError::login_rate_limited(),
                StatusCode::TOO_MANY_REQUESTS,
                42901,
                "login_rate_limited",
            ),
            (
                AppError::mfa_already_enabled(),
                StatusCode::UNPROCESSABLE_ENTITY,
                42206,
                "mfa_already_enabled",
            ),
            (
                AppError::mfa_failed(),
                StatusCode::UNAUTHORIZED,
                40104,
                "mfa_failed",
            ),
            (
                AppError::mfa_not_enabled(),
                StatusCode::UNPROCESSABLE_ENTITY,
                42207,
                "mfa_not_enabled",
            ),
            (
                AppError::mfa_required(),
                StatusCode::UNAUTHORIZED,
                40103,
                "mfa_required",
            ),
            (
                AppError::not_found("not found"),
                StatusCode::NOT_FOUND,
                40400,
                "not_found",
            ),
            (
                AppError::password_reset_rate_limited(),
                StatusCode::TOO_MANY_REQUESTS,
                42902,
                "password_reset_rate_limited",
            ),
            (
                AppError::password_reset_token_invalid(),
                StatusCode::UNAUTHORIZED,
                40109,
                "password_reset_token_invalid",
            ),
            (
                AppError::rate_limited(),
                StatusCode::TOO_MANY_REQUESTS,
                42900,
                "rate_limited",
            ),
            (
                AppError::refresh_rate_limited(),
                StatusCode::TOO_MANY_REQUESTS,
                42904,
                "refresh_rate_limited",
            ),
            (
                AppError::refresh_reuse_detected(),
                StatusCode::UNAUTHORIZED,
                40108,
                "refresh_reuse_detected",
            ),
            (
                AppError::release_not_found(),
                StatusCode::NOT_FOUND,
                40406,
                "release_not_found",
            ),
            (
                AppError::session_expired(),
                StatusCode::UNAUTHORIZED,
                40101,
                "session_expired",
            ),
            (
                AppError::signature_invalid("signature invalid"),
                StatusCode::FORBIDDEN,
                40308,
                "signature_invalid",
            ),
            (
                AppError::signature_required("signature required"),
                StatusCode::FORBIDDEN,
                40307,
                "signature_required",
            ),
            (
                AppError::script_not_found(),
                StatusCode::NOT_FOUND,
                40407,
                "script_not_found",
            ),
            (
                AppError::subscription_inactive("subscription is inactive"),
                StatusCode::FORBIDDEN,
                40306,
                "subscription_inactive",
            ),
            (
                AppError::tenant_forbidden(),
                StatusCode::FORBIDDEN,
                40301,
                "tenant_forbidden",
            ),
            (
                AppError::tenant_not_found(),
                StatusCode::NOT_FOUND,
                40401,
                "tenant_not_found",
            ),
            (
                AppError::token_expired(),
                StatusCode::UNAUTHORIZED,
                40106,
                "token_expired",
            ),
            (
                AppError::token_invalid("token invalid"),
                StatusCode::UNAUTHORIZED,
                40107,
                "token_invalid",
            ),
            (
                AppError::unauthenticated(),
                StatusCode::UNAUTHORIZED,
                40100,
                "unauthenticated",
            ),
            (
                AppError::user_not_found(),
                StatusCode::NOT_FOUND,
                40402,
                "user_not_found",
            ),
            (
                AppError::validation_failed("validation failed"),
                StatusCode::BAD_REQUEST,
                40001,
                "validation_failed",
            ),
            (
                AppError::weak_password(),
                StatusCode::UNPROCESSABLE_ENTITY,
                42201,
                "weak_password",
            ),
            (
                AppError::activation_rate_limited(),
                StatusCode::TOO_MANY_REQUESTS,
                42903,
                "activation_rate_limited",
            ),
        ];

        for (error, expected_status, expected_code, expected_message) in cases {
            let response = error.into_response();
            assert_eq!(response.status(), expected_status);

            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("response body");
            let body: Value = serde_json::from_slice(&body).expect("json body");
            assert_eq!(body["code"], expected_code);
            assert_eq!(body["message"], expected_message);
            assert_eq!(body["errorCode"], expected_message);

            let documented_pair = format!("{expected_code} {expected_message}");
            assert!(
                documented_errors.contains(&documented_pair),
                "{documented_pair} is missing from 权限点与错误码清单.md"
            );
        }
    }

    #[tokio::test]
    async fn app_error_response_exposes_stable_detail_error_code() {
        let response = AppError::validation_failed(
            "model_not_support_reference_video: model does not support reference video",
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(body["code"], 40001);
        assert_eq!(body["message"], "validation_failed");
        assert_eq!(body["errorCode"], "model_not_support_reference_video");
    }

    #[tokio::test]
    async fn app_error_response_exposes_reference_limit_error_codes() {
        let response = AppError::validation_failed(
            "reference_video_too_many: reference videos must contain no more than 1 items",
        )
        .into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(body["code"], 40001);
        assert_eq!(body["message"], "validation_failed");
        assert_eq!(body["errorCode"], "reference_video_too_many");
    }
}
