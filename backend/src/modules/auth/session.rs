use axum::{
    extract::{Request, State},
    http::{header::COOKIE, HeaderValue},
    middleware::Next,
    response::Response,
};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    crypto::token::{hash_token, verify_token_hash},
    error::AppError,
    modules::iam::{permission::PermissionRepository, role::RoleRepository},
    state::AppState,
};

pub const ADMIN_SESSION_COOKIE: &str = "admin_session";
pub const ADMIN_REFRESH_COOKIE: &str = "admin_refresh";

#[derive(Debug, Clone)]
pub struct AdminContext {
    pub session_id: Uuid,
    pub tenant_id: Uuid,
    pub team_member_id: Uuid,
    pub email: String,
    pub name: String,
    pub email_verified: bool,
    pub mfa_enabled: bool,
    pub tenant_name: String,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, FromRow)]
struct SessionCandidate {
    session_id: Uuid,
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: String,
    name: String,
    email_verified: bool,
    mfa_enabled: bool,
    member_status: String,
    tenant_name: String,
    tenant_status: String,
}

pub async fn require_admin_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let session_id =
        read_signed_session_cookie(request.headers(), &state.config.security.session_secret)?
            .and_then(|value| Uuid::parse_str(&value).ok())
            .ok_or_else(AppError::unauthenticated)?;

    let candidate = load_session(&state, session_id)
        .await?
        .ok_or_else(AppError::session_expired)?;

    if candidate.member_status != "active" {
        return Err(AppError::account_disabled());
    }

    if candidate.tenant_status != "active" {
        return Err(AppError::tenant_forbidden());
    }

    let permissions = PermissionRepository::new(state.db.clone())
        .list_for_member(candidate.tenant_id, candidate.team_member_id)
        .await?;
    let roles = RoleRepository::new(state.db.clone())
        .list_codes_for_member(candidate.tenant_id, candidate.team_member_id)
        .await?;

    request.extensions_mut().insert(AdminContext {
        session_id: candidate.session_id,
        tenant_id: candidate.tenant_id,
        team_member_id: candidate.team_member_id,
        email: candidate.email,
        name: candidate.name,
        email_verified: candidate.email_verified,
        mfa_enabled: candidate.mfa_enabled,
        tenant_name: candidate.tenant_name,
        roles,
        permissions,
    });

    Ok(next.run(request).await)
}

pub fn read_cookie<'a>(headers: &'a axum::http::HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

pub fn build_session_cookie(
    session_id: Uuid,
    session_secret: &str,
    secure: bool,
    max_age_seconds: i64,
) -> Result<HeaderValue, AppError> {
    let signed_session_id = sign_cookie_value(session_secret, &session_id.to_string())?;

    cookie_header(format!(
        "{ADMIN_SESSION_COOKIE}={signed_session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age={max_age_seconds}{}",
        secure_attr(secure)
    ))
}

pub fn build_refresh_cookie(
    refresh_token: &str,
    secure: bool,
    max_age_seconds: i64,
) -> Result<HeaderValue, AppError> {
    cookie_header(format!(
        "{ADMIN_REFRESH_COOKIE}={refresh_token}; HttpOnly; SameSite=Lax; Path=/api/auth/refresh; Max-Age={max_age_seconds}{}",
        secure_attr(secure)
    ))
}

pub fn build_clear_session_cookie(secure: bool) -> Result<HeaderValue, AppError> {
    cookie_header(format!(
        "{ADMIN_SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0{}",
        secure_attr(secure)
    ))
}

pub fn build_clear_refresh_cookie(secure: bool) -> Result<HeaderValue, AppError> {
    cookie_header(format!(
        "{ADMIN_REFRESH_COOKIE}=; HttpOnly; SameSite=Lax; Path=/api/auth/refresh; Max-Age=0{}",
        secure_attr(secure)
    ))
}

fn secure_attr(secure: bool) -> &'static str {
    if secure {
        "; Secure"
    } else {
        ""
    }
}

fn cookie_header(value: String) -> Result<HeaderValue, AppError> {
    HeaderValue::from_str(&value)
        .map_err(|error| AppError::config(format!("invalid session cookie: {error}")))
}

fn read_signed_session_cookie(
    headers: &axum::http::HeaderMap,
    session_secret: &str,
) -> Result<Option<String>, AppError> {
    let Some(value) = read_cookie(headers, ADMIN_SESSION_COOKIE) else {
        return Ok(None);
    };

    verify_cookie_value(session_secret, value)
}

fn sign_cookie_value(secret: &str, value: &str) -> Result<String, AppError> {
    let signature = hash_token(secret, value)?;

    Ok(format!("{value}.{signature}"))
}

fn verify_cookie_value(secret: &str, signed_value: &str) -> Result<Option<String>, AppError> {
    let Some((value, signature)) = signed_value.rsplit_once('.') else {
        return Ok(None);
    };
    if value.is_empty() || signature.is_empty() {
        return Ok(None);
    }
    if verify_token_hash(secret, value, signature)? {
        return Ok(Some(value.to_owned()));
    }

    Ok(None)
}

async fn load_session(
    state: &AppState,
    session_id: Uuid,
) -> Result<Option<SessionCandidate>, AppError> {
    sqlx::query_as::<_, SessionCandidate>(
        r#"
        select
          s.id as session_id,
          s.tenant_id,
          s.team_member_id,
          tm.email,
          tm.name,
          tm.email_verified,
          tm.mfa_enabled,
          tm.status as member_status,
          t.name as tenant_name,
          t.status as tenant_status
        from admin_sessions s
        join team_members tm
          on tm.id = s.team_member_id
         and tm.tenant_id = s.tenant_id
         and tm.deleted_at is null
        join tenants t
          on t.id = s.tenant_id
         and t.deleted_at is null
        where s.id = $1
          and s.revoked_at is null
          and s.expires_at > now()
        "#,
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|error| AppError::dependency(format!("admin session query failed: {error}")))
}

#[cfg(test)]
mod tests {
    use axum::http::{header::COOKIE, HeaderMap, HeaderValue};
    use uuid::Uuid;

    use super::{
        build_clear_refresh_cookie, build_clear_session_cookie, build_refresh_cookie,
        build_session_cookie, read_cookie, sign_cookie_value, verify_cookie_value,
        ADMIN_REFRESH_COOKIE,
    };

    #[test]
    fn read_cookie_finds_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("theme=dark; admin_session=abc; other=1"),
        );

        assert_eq!(read_cookie(&headers, "admin_session"), Some("abc"));
    }

    #[test]
    fn read_cookie_returns_none_when_missing() {
        let headers = HeaderMap::new();

        assert_eq!(read_cookie(&headers, "admin_session"), None);
    }

    #[test]
    fn session_cookie_uses_http_only_same_site_and_max_age() {
        let session_id = Uuid::nil();
        let header = build_session_cookie(session_id, "session-secret", true, 86_400)
            .expect("session cookie should be valid");
        let cookie = header.to_str().expect("session cookie should be visible");

        assert!(cookie.contains("admin_session=00000000-0000-0000-0000-000000000000"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=86400"));
        assert!(cookie.contains("Secure"));
    }

    #[test]
    fn signed_session_cookie_round_trips() {
        let value = Uuid::nil().to_string();
        let signed = sign_cookie_value("session-secret", &value).expect("signed cookie");

        assert_eq!(
            verify_cookie_value("session-secret", &signed).expect("verify signed cookie"),
            Some(value)
        );
        assert_eq!(
            verify_cookie_value("other-secret", &signed).expect("reject signed cookie"),
            None
        );
        assert_eq!(
            verify_cookie_value("session-secret", "unsigned-cookie").expect("reject unsigned"),
            None
        );
    }

    #[test]
    fn refresh_cookie_is_scoped_to_refresh_endpoint() {
        let header = build_refresh_cookie("refresh-token", true, 86_400).expect("refresh cookie");
        let cookie = header.to_str().expect("refresh cookie should be visible");

        assert!(cookie.contains(&format!("{ADMIN_REFRESH_COOKIE}=refresh-token")));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/api/auth/refresh"));
        assert!(cookie.contains("Max-Age=86400"));
        assert!(cookie.contains("Secure"));
    }

    #[test]
    fn clear_session_cookie_expires_cookie() {
        let header = build_clear_session_cookie(false).expect("clear cookie should be valid");
        let cookie = header.to_str().expect("clear cookie should be visible");

        assert!(cookie.starts_with("admin_session=;"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=0"));
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn clear_refresh_cookie_expires_cookie() {
        let header = build_clear_refresh_cookie(false).expect("clear refresh cookie");
        let cookie = header
            .to_str()
            .expect("clear refresh cookie should be visible");

        assert!(cookie.starts_with("admin_refresh=;"));
        assert!(cookie.contains("Path=/api/auth/refresh"));
        assert!(cookie.contains("Max-Age=0"));
        assert!(!cookie.contains("Secure"));
    }
}
