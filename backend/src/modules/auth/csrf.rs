use axum::{
    extract::{Request, State},
    http::{HeaderValue, Method},
    middleware::Next,
    response::Response,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::{
    crypto::token::generate_token, error::AppError, modules::auth::session::read_cookie,
    state::AppState,
};

pub const ADMIN_CSRF_COOKIE: &str = "admin_csrf";
pub const ADMIN_CSRF_HEADER: &str = "x-csrf-token";

type HmacSha256 = Hmac<Sha256>;

pub async fn require_csrf(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !requires_csrf(request.method()) {
        return Ok(next.run(request).await);
    }

    let cookie_token = read_cookie(request.headers(), ADMIN_CSRF_COOKIE)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(AppError::csrf_failed)?;
    let header_token = request
        .headers()
        .get(ADMIN_CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(AppError::csrf_failed)?;

    if !csrf_tokens_are_valid(
        &state.config.security.csrf_secret,
        cookie_token,
        header_token,
    )? {
        return Err(AppError::csrf_failed());
    }

    Ok(next.run(request).await)
}

pub fn issue_csrf_token(secret: &str) -> Result<String, AppError> {
    let nonce = generate_token();
    let signature = sign_nonce(secret, &nonce)?;

    Ok(format!("{nonce}.{signature}"))
}

pub fn build_csrf_cookie(
    token: &str,
    secure: bool,
    max_age_seconds: i64,
) -> Result<HeaderValue, AppError> {
    cookie_header(format!(
        "{ADMIN_CSRF_COOKIE}={token}; SameSite=Lax; Path=/; Max-Age={max_age_seconds}{}",
        secure_attr(secure)
    ))
}

pub fn build_clear_csrf_cookie(secure: bool) -> Result<HeaderValue, AppError> {
    cookie_header(format!(
        "{ADMIN_CSRF_COOKIE}=; SameSite=Lax; Path=/; Max-Age=0{}",
        secure_attr(secure)
    ))
}

fn csrf_tokens_are_valid(
    secret: &str,
    cookie_token: &str,
    header_token: &str,
) -> Result<bool, AppError> {
    if !constant_time_eq(cookie_token, header_token) {
        return Ok(false);
    }

    verify_csrf_token(secret, header_token)
}

fn verify_csrf_token(secret: &str, token: &str) -> Result<bool, AppError> {
    let Some((nonce, signature)) = token.split_once('.') else {
        return Ok(false);
    };
    if nonce.is_empty() || signature.is_empty() {
        return Ok(false);
    }

    let expected_signature = sign_nonce(secret, nonce)?;

    Ok(constant_time_eq(signature, &expected_signature))
}

fn sign_nonce(secret: &str, nonce: &str) -> Result<String, AppError> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|error| AppError::crypto(format!("csrf secret invalid: {error}")))?;
    mac.update(nonce.as_bytes());

    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

fn requires_csrf(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    left.as_bytes().ct_eq(right.as_bytes()).into()
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
        .map_err(|error| AppError::config(format!("invalid csrf cookie: {error}")))
}

#[cfg(test)]
mod tests {
    use axum::http::Method;

    use super::{
        build_clear_csrf_cookie, build_csrf_cookie, csrf_tokens_are_valid, issue_csrf_token,
        requires_csrf, verify_csrf_token, ADMIN_CSRF_COOKIE,
    };

    #[test]
    fn csrf_token_round_trips_with_same_secret() {
        let token = issue_csrf_token("csrf-secret").expect("csrf token");

        assert!(verify_csrf_token("csrf-secret", &token).expect("verify csrf"));
        assert!(!verify_csrf_token("other-secret", &token).expect("reject csrf"));
    }

    #[test]
    fn csrf_requires_matching_cookie_and_header_tokens() {
        let token = issue_csrf_token("csrf-secret").expect("csrf token");

        assert!(csrf_tokens_are_valid("csrf-secret", &token, &token).expect("valid csrf"));
        assert!(
            !csrf_tokens_are_valid("csrf-secret", &token, "other-token").expect("mismatch csrf")
        );
    }

    #[test]
    fn csrf_rejects_tampered_signature() {
        let token = issue_csrf_token("csrf-secret").expect("csrf token");
        let tampered = format!("{token}x");

        assert!(!verify_csrf_token("csrf-secret", &tampered).expect("tampered csrf"));
    }

    #[test]
    fn safe_methods_do_not_require_csrf() {
        assert!(!requires_csrf(&Method::GET));
        assert!(!requires_csrf(&Method::HEAD));
        assert!(!requires_csrf(&Method::OPTIONS));
        assert!(requires_csrf(&Method::POST));
        assert!(requires_csrf(&Method::PUT));
        assert!(requires_csrf(&Method::DELETE));
    }

    #[test]
    fn csrf_cookie_is_readable_by_frontend_and_same_site() {
        let header = build_csrf_cookie("token", true, 86_400).expect("csrf cookie");
        let cookie = header.to_str().expect("csrf cookie should be visible");

        assert!(cookie.starts_with(&format!("{ADMIN_CSRF_COOKIE}=token;")));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=86400"));
        assert!(cookie.contains("Secure"));
        assert!(!cookie.contains("HttpOnly"));
    }

    #[test]
    fn clear_csrf_cookie_expires_cookie() {
        let header = build_clear_csrf_cookie(false).expect("clear csrf cookie");
        let cookie = header
            .to_str()
            .expect("clear csrf cookie should be visible");

        assert!(cookie.starts_with(&format!("{ADMIN_CSRF_COOKIE}=;")));
        assert!(cookie.contains("Max-Age=0"));
        assert!(!cookie.contains("Secure"));
    }
}
