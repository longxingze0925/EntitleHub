use serde::Serialize;

use crate::{
    ai::{
        build_chat_completions_body, build_embeddings_body, build_image_generations_body,
        build_video_generations_body,
    },
    auth::{
        build_email_verify_confirm_request, build_heartbeat_request,
        build_password_reset_confirm_request, build_refresh_request, ActivationRequestPayload,
        CustomerLoginRequestPayload,
    },
    cache::SdkCacheEnvelope,
    device::{build_rotate_device_key_request, DeviceIdentity},
    request::{build_authorized_cached_device_request, CachedAuthorizedDeviceRequestInput},
    script::build_fetch_secure_script_request,
    session::{ClientSessionState, SessionManager, SessionRefresh},
    SdkError, SdkResult,
};

pub const ACTIVATE_PATH: &str = "/api/client/auth/activate";
pub const LOGIN_PATH: &str = "/api/client/auth/login";
pub const REFRESH_PATH: &str = "/api/client/auth/refresh";
pub const HEARTBEAT_PATH: &str = "/api/client/auth/heartbeat";
pub const VERIFY_PATH: &str = "/api/client/auth/verify";
pub const LOGOUT_PATH: &str = "/api/client/auth/logout";
pub const EMAIL_VERIFY_REQUEST_PATH: &str = "/api/client/auth/email/verify/request";
pub const EMAIL_VERIFY_CONFIRM_PATH: &str = "/api/client/auth/email/verify/confirm";
pub const PASSWORD_RESET_CONFIRM_PATH: &str = "/api/client/auth/password/reset/confirm";
pub const RELEASE_LATEST_PATH: &str = "/api/client/releases/latest";
pub const SECURE_SCRIPT_VERSIONS_PATH: &str = "/api/client/secure-scripts/versions";
pub const SECURE_SCRIPT_FETCH_PATH: &str = "/api/client/secure-scripts/fetch";
pub const DEVICE_SELF_PATH: &str = "/api/client/devices/self";
pub const DEVICE_ROTATE_KEY_PATH: &str = "/api/client/devices/self/rotate-key";
pub const AI_MODELS_PATH: &str = "/api/client/ai/v1/models";
pub const AI_CHAT_COMPLETIONS_PATH: &str = "/api/client/ai/v1/chat/completions";
pub const AI_IMAGE_GENERATIONS_PATH: &str = "/api/client/ai/v1/images/generations";
pub const AI_VIDEO_GENERATIONS_PATH: &str = "/api/client/ai/v1/videos/generations";
pub const AI_EMBEDDINGS_PATH: &str = "/api/client/ai/v1/embeddings";

const JSON_CONTENT_TYPE: &str = "application/json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientConfig {
    pub base_url: String,
    pub app_key: String,
    pub refresh_before_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRequestParts {
    pub method: String,
    pub path: String,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProtectedClientRequestContext<'a> {
    pub cache: &'a SdkCacheEnvelope,
    pub session_manager: &'a SessionManager,
    pub timestamp: i64,
    pub nonce: &'a str,
    pub refresh_before_seconds: i64,
}

impl ClientConfig {
    pub fn new(base_url: &str, app_key: &str) -> SdkResult<Self> {
        let base_url = base_url.trim().trim_end_matches('/');
        let app_key = app_key.trim();
        if base_url.is_empty() {
            return Err(SdkError::InvalidClientRequest("base_url"));
        }
        if app_key.is_empty() {
            return Err(SdkError::InvalidClientRequest("app_key"));
        }

        Ok(Self {
            base_url: base_url.to_owned(),
            app_key: app_key.to_owned(),
            refresh_before_seconds: 60,
        })
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

impl ClientRequestParts {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
    }
}

pub fn jwks_path(app_key: &str) -> SdkResult<String> {
    Ok(format!(
        "/api/client/apps/{}/jwks",
        clean_path_segment("app_key", app_key)?
    ))
}

pub fn release_download_path(file_name: &str) -> SdkResult<String> {
    Ok(format!(
        "/api/client/releases/download/{}",
        clean_path_segment("file_name", file_name)?
    ))
}

pub fn ai_asset_path(asset_id: &str) -> SdkResult<String> {
    Ok(format!(
        "/api/ai/assets/{}",
        clean_path_segment("asset_id", asset_id)?
    ))
}

pub fn jwks_request(app_key: &str) -> SdkResult<ClientRequestParts> {
    Ok(empty_request("GET", jwks_path(app_key)?))
}

pub fn release_download_request(file_name: &str) -> SdkResult<ClientRequestParts> {
    Ok(empty_request("GET", release_download_path(file_name)?))
}

pub fn activation_request(payload: &ActivationRequestPayload) -> SdkResult<ClientRequestParts> {
    public_json_request("POST", ACTIVATE_PATH, payload)
}

pub fn customer_login_request(
    payload: &CustomerLoginRequestPayload,
) -> SdkResult<ClientRequestParts> {
    public_json_request("POST", LOGIN_PATH, payload)
}

pub fn refresh_request(refresh_token: &str) -> SdkResult<ClientRequestParts> {
    let payload = build_refresh_request(refresh_token)?;
    public_json_request("POST", REFRESH_PATH, &payload)
}

pub fn email_verify_confirm_request(token: &str) -> SdkResult<ClientRequestParts> {
    let payload = build_email_verify_confirm_request(token)?;
    public_json_request("POST", EMAIL_VERIFY_CONFIRM_PATH, &payload)
}

pub fn password_reset_confirm_request(
    token: &str,
    new_password: &str,
) -> SdkResult<ClientRequestParts> {
    let payload = build_password_reset_confirm_request(token, new_password)?;
    public_json_request("POST", PASSWORD_RESET_CONFIRM_PATH, &payload)
}

pub fn heartbeat_request<F>(
    context: ProtectedClientRequestContext<'_>,
    app_version: Option<&str>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let payload = build_heartbeat_request(app_version);
    protected_json_request(
        context,
        "POST",
        HEARTBEAT_PATH,
        &payload,
        Vec::new(),
        refresh,
    )
}

pub fn verify_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "POST", VERIFY_PATH, refresh)
}

pub fn logout_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "POST", LOGOUT_PATH, refresh)
}

pub fn latest_release_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "GET", RELEASE_LATEST_PATH, refresh)
}

pub fn secure_script_versions_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "GET", SECURE_SCRIPT_VERSIONS_PATH, refresh)
}

pub fn fetch_secure_script_request<F>(
    context: ProtectedClientRequestContext<'_>,
    script_id: &str,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let payload = build_fetch_secure_script_request(script_id)?;
    protected_json_request(
        context,
        "POST",
        SECURE_SCRIPT_FETCH_PATH,
        &payload,
        Vec::new(),
        refresh,
    )
}

pub fn email_verify_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "POST", EMAIL_VERIFY_REQUEST_PATH, refresh)
}

pub fn unbind_current_device_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "DELETE", DEVICE_SELF_PATH, refresh)
}

pub fn rotate_device_key_request<F>(
    context: ProtectedClientRequestContext<'_>,
    next_device: &DeviceIdentity,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let payload = build_rotate_device_key_request(next_device)?;
    protected_json_request(
        context,
        "POST",
        DEVICE_ROTATE_KEY_PATH,
        &payload,
        Vec::new(),
        refresh,
    )
}

pub fn ai_models_request<F>(
    context: ProtectedClientRequestContext<'_>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_empty_request(context, "GET", AI_MODELS_PATH, refresh)
}

pub fn ai_chat_completions_request<F>(
    context: ProtectedClientRequestContext<'_>,
    payload: &serde_json::Value,
    idempotency_key: Option<&str>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let body = build_chat_completions_body(payload)?;
    protected_request(
        context,
        "POST",
        AI_CHAT_COMPLETIONS_PATH,
        body,
        ai_extra_headers(idempotency_key)?,
        refresh,
    )
}

pub fn ai_image_generations_request<F>(
    context: ProtectedClientRequestContext<'_>,
    payload: &serde_json::Value,
    idempotency_key: Option<&str>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let body = build_image_generations_body(payload)?;
    protected_request(
        context,
        "POST",
        AI_IMAGE_GENERATIONS_PATH,
        body,
        ai_extra_headers(idempotency_key)?,
        refresh,
    )
}

pub fn ai_video_generations_request<F>(
    context: ProtectedClientRequestContext<'_>,
    payload: &serde_json::Value,
    idempotency_key: Option<&str>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let body = build_video_generations_body(payload)?;
    protected_request(
        context,
        "POST",
        AI_VIDEO_GENERATIONS_PATH,
        body,
        ai_extra_headers(idempotency_key)?,
        refresh,
    )
}

pub fn ai_embeddings_request<F>(
    context: ProtectedClientRequestContext<'_>,
    payload: &serde_json::Value,
    idempotency_key: Option<&str>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let body = build_embeddings_body(payload)?;
    protected_request(
        context,
        "POST",
        AI_EMBEDDINGS_PATH,
        body,
        ai_extra_headers(idempotency_key)?,
        refresh,
    )
}

fn public_json_request<T>(
    method: &str,
    path: &'static str,
    payload: &T,
) -> SdkResult<ClientRequestParts>
where
    T: Serialize,
{
    let body = serde_json::to_vec(payload).map_err(|_| SdkError::InvalidClientRequest(path))?;
    let mut request = ClientRequestParts {
        method: method.to_owned(),
        path: path.to_owned(),
        body,
        headers: Vec::new(),
    };
    push_json_content_type(&mut request.headers);

    Ok(request)
}

fn protected_json_request<T, F>(
    context: ProtectedClientRequestContext<'_>,
    method: &'static str,
    path: &'static str,
    payload: &T,
    extra_headers: Vec<(String, String)>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    T: Serialize,
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let body = serde_json::to_vec(payload).map_err(|_| SdkError::InvalidClientRequest(path))?;
    protected_request(context, method, path, body, extra_headers, refresh)
}

fn protected_empty_request<F>(
    context: ProtectedClientRequestContext<'_>,
    method: &'static str,
    path: &'static str,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    protected_request(context, method, path, Vec::new(), Vec::new(), refresh)
}

fn protected_request<F>(
    context: ProtectedClientRequestContext<'_>,
    method: &'static str,
    path: &'static str,
    body: Vec<u8>,
    mut extra_headers: Vec<(String, String)>,
    refresh: F,
) -> SdkResult<ClientRequestParts>
where
    F: FnOnce(&ClientSessionState) -> SdkResult<SessionRefresh>,
{
    let mut headers = build_authorized_cached_device_request(
        context.cache,
        context.session_manager,
        CachedAuthorizedDeviceRequestInput {
            method,
            path,
            body: &body,
            timestamp: context.timestamp,
            nonce: context.nonce,
            refresh_before_seconds: context.refresh_before_seconds,
        },
        refresh,
    )?
    .headers;
    if !body.is_empty() {
        push_json_content_type(&mut headers);
    }
    headers.append(&mut extra_headers);

    Ok(ClientRequestParts {
        method: method.to_owned(),
        path: path.to_owned(),
        body,
        headers,
    })
}

fn empty_request(method: &str, path: String) -> ClientRequestParts {
    ClientRequestParts {
        method: method.to_owned(),
        path,
        body: Vec::new(),
        headers: Vec::new(),
    }
}

fn ai_extra_headers(idempotency_key: Option<&str>) -> SdkResult<Vec<(String, String)>> {
    let Some(idempotency_key) = idempotency_key else {
        return Ok(Vec::new());
    };
    let idempotency_key = clean_header_value("idempotency-key", idempotency_key)?;

    Ok(vec![("Idempotency-Key".to_owned(), idempotency_key)])
}

fn push_json_content_type(headers: &mut Vec<(String, String)>) {
    if headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("Content-Type"))
    {
        return;
    }
    headers.push(("Content-Type".to_owned(), JSON_CONTENT_TYPE.to_owned()));
}

fn clean_header_value(field: &'static str, value: &str) -> SdkResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 200
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(SdkError::InvalidClientRequest(field));
    }

    Ok(value.to_owned())
}

fn clean_path_segment(field: &'static str, value: &str) -> SdkResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
        || value.contains('?')
        || value.contains('#')
        || value.chars().any(char::is_control)
    {
        return Err(SdkError::InvalidClientRequest(field));
    }

    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        auth::ClientBootstrap,
        cache::SdkCacheEnvelope,
        device::DeviceIdentity,
        jwks::JwksCache,
        session::{ClientSessionState, SessionInit, SessionManager, SessionRefresh},
    };

    use super::{
        activation_request, ai_chat_completions_request, ai_embeddings_request,
        ai_image_generations_request, ai_models_request, ai_video_generations_request,
        email_verify_confirm_request, fetch_secure_script_request, heartbeat_request, jwks_path,
        jwks_request, latest_release_request, password_reset_confirm_request, refresh_request,
        release_download_path, rotate_device_key_request, secure_script_versions_request,
        unbind_current_device_request, verify_request, ClientConfig, ProtectedClientRequestContext,
        AI_CHAT_COMPLETIONS_PATH, AI_EMBEDDINGS_PATH, AI_IMAGE_GENERATIONS_PATH, AI_MODELS_PATH,
        AI_VIDEO_GENERATIONS_PATH, DEVICE_ROTATE_KEY_PATH, HEARTBEAT_PATH, RELEASE_LATEST_PATH,
        SECURE_SCRIPT_FETCH_PATH, SECURE_SCRIPT_VERSIONS_PATH, VERIFY_PATH,
    };

    #[test]
    fn public_requests_build_paths_headers_and_bodies() {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device");
        let bootstrap = ClientBootstrap::new(device).expect("bootstrap");
        let activation = bootstrap
            .activation_request("app_key", "license", None, None, None)
            .expect("activation payload");
        let request = activation_request(&activation).expect("activation request");

        assert_eq!(request.method, "POST");
        assert_eq!(request.path, super::ACTIVATE_PATH);
        assert_eq!(request.header("Content-Type"), Some("application/json"));
        assert!(String::from_utf8(request.body)
            .expect("body")
            .contains("\"license_key\""));

        assert_eq!(
            jwks_path(" app_key ").expect("jwks path"),
            "/api/client/apps/app_key/jwks"
        );
        assert!(jwks_path("../app").is_err());
        assert_eq!(jwks_request("app_key").expect("jwks request").method, "GET");
        assert_eq!(
            release_download_path("app.zip").expect("download path"),
            "/api/client/releases/download/app.zip"
        );
        assert!(release_download_path("../app.zip").is_err());

        assert!(refresh_request("refresh-token")
            .expect("refresh request")
            .body
            .windows("refresh-token".len())
            .any(|window| window == b"refresh-token"));
        assert!(email_verify_confirm_request("token").is_ok());
        assert!(password_reset_confirm_request("token", "Password@123").is_ok());

        let config = ClientConfig::new("https://example.com/", "app_key").expect("config");
        assert_eq!(
            config.url(super::LOGIN_PATH),
            "https://example.com/api/client/auth/login"
        );
    }

    #[test]
    fn protected_requests_sign_every_client_endpoint() {
        let (cache, manager) = fixture_cache_and_manager();
        let context = ProtectedClientRequestContext {
            cache: &cache,
            session_manager: &manager,
            timestamp: 200,
            nonce: "0123456789abcdef",
            refresh_before_seconds: 60,
        };
        let next_device = cache.device.rotate_key().expect("next device");

        let requests = vec![
            heartbeat_request(context, Some("1.0.0"), no_refresh).expect("heartbeat"),
            verify_request(context, no_refresh).expect("verify"),
            latest_release_request(context, no_refresh).expect("latest release"),
            secure_script_versions_request(context, no_refresh).expect("script versions"),
            fetch_secure_script_request(
                context,
                "00000000-0000-0000-0000-000000000010",
                no_refresh,
            )
            .expect("script fetch"),
            unbind_current_device_request(context, no_refresh).expect("unbind"),
            rotate_device_key_request(context, &next_device, no_refresh).expect("rotate"),
            ai_models_request(context, no_refresh).expect("ai models"),
        ];
        let paths = requests
            .iter()
            .map(|request| request.path.as_str())
            .collect::<Vec<_>>();

        assert!(paths.contains(&HEARTBEAT_PATH));
        assert!(paths.contains(&VERIFY_PATH));
        assert!(paths.contains(&RELEASE_LATEST_PATH));
        assert!(paths.contains(&SECURE_SCRIPT_VERSIONS_PATH));
        assert!(paths.contains(&SECURE_SCRIPT_FETCH_PATH));
        assert!(paths.contains(&super::DEVICE_SELF_PATH));
        assert!(paths.contains(&DEVICE_ROTATE_KEY_PATH));
        assert!(paths.contains(&AI_MODELS_PATH));
        for request in requests {
            assert_eq!(request.header("Authorization"), Some("Bearer access-token"));
            assert_eq!(request.header("X-Device-Key-Id"), Some("device-key-id"));
        }
    }

    #[test]
    fn ai_requests_sign_payload_and_optional_idempotency_key() {
        let (cache, manager) = fixture_cache_and_manager();
        let context = ProtectedClientRequestContext {
            cache: &cache,
            session_manager: &manager,
            timestamp: 200,
            nonce: "0123456789abcdef",
            refresh_before_seconds: 60,
        };

        let chat = ai_chat_completions_request(
            context,
            &json!({
                "model": "gpt-test",
                "messages": [{ "role": "user", "content": "hello" }]
            }),
            Some("request-1"),
            no_refresh,
        )
        .expect("chat request");
        let image = ai_image_generations_request(
            context,
            &json!({ "model": "image-test", "prompt": "logo" }),
            None,
            no_refresh,
        )
        .expect("image request");
        let video = ai_video_generations_request(
            context,
            &json!({ "model": "video-test", "prompt": "intro", "duration": 8 }),
            None,
            no_refresh,
        )
        .expect("video request");
        let embeddings = ai_embeddings_request(
            context,
            &json!({ "model": "embed-test", "input": "hello" }),
            None,
            no_refresh,
        )
        .expect("embeddings request");

        assert_eq!(chat.path, AI_CHAT_COMPLETIONS_PATH);
        assert_eq!(chat.header("Idempotency-Key"), Some("request-1"));
        assert_eq!(chat.header("Content-Type"), Some("application/json"));
        assert_eq!(image.path, AI_IMAGE_GENERATIONS_PATH);
        assert_eq!(video.path, AI_VIDEO_GENERATIONS_PATH);
        assert_eq!(embeddings.path, AI_EMBEDDINGS_PATH);
        assert!(ai_chat_completions_request(
            context,
            &json!({ "model": "gpt-test", "stream": true }),
            None,
            no_refresh,
        )
        .is_err());
    }

    fn fixture_cache_and_manager() -> (SdkCacheEnvelope, SessionManager) {
        let device = DeviceIdentity::generate("app_key", &["machine"]).expect("device identity");
        let session = ClientSessionState::from_init(
            SessionInit {
                session_id: "session-id".to_owned(),
                device_id: "device-id".to_owned(),
                access_token: "access-token".to_owned(),
                refresh_token: "refresh-token".to_owned(),
                token_type: None,
                expires_in: 900,
                refresh_expires_in: 2_500,
                features: json!({}),
            },
            100,
        )
        .expect("session");
        let manager = SessionManager::new(Some(session.clone()));
        let cache = SdkCacheEnvelope::new_with_device_key_id(
            "app_key",
            device,
            Some("device-key-id"),
            Some(session),
            &JwksCache::default(),
            100,
        )
        .expect("cache");

        (cache, manager)
    }

    fn no_refresh(_session: &ClientSessionState) -> crate::SdkResult<SessionRefresh> {
        unreachable!("access token should not refresh")
    }
}
