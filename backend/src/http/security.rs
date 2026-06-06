use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

const ALLOW_METHODS: &str = "GET,POST,PUT,PATCH,DELETE,OPTIONS";
const ALLOW_HEADERS: &str = "Content-Type,Authorization,X-CSRF-Token,X-Device-Id,X-Device-Key-Id,X-Timestamp,X-Nonce,X-Body-SHA256,X-Signature";
const MAX_AGE_SECONDS: &str = "600";

pub async fn apply_security(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let origin = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let allowed_origin = origin
        .as_deref()
        .filter(|origin| origin_is_allowed(&state, origin))
        .map(str::to_owned);

    if is_cors_preflight(&request) {
        let mut response = if allowed_origin.is_some() {
            StatusCode::NO_CONTENT.into_response()
        } else {
            StatusCode::FORBIDDEN.into_response()
        };
        apply_security_headers(response.headers_mut(), state.config.app.env == "production");
        apply_cors_headers(response.headers_mut(), allowed_origin.as_deref());

        return response;
    }

    let mut response = next.run(request).await;
    apply_security_headers(response.headers_mut(), state.config.app.env == "production");
    apply_cors_headers(response.headers_mut(), allowed_origin.as_deref());

    response
}

fn is_cors_preflight(request: &Request) -> bool {
    request.method() == Method::OPTIONS
        && request
            .headers()
            .contains_key("access-control-request-method")
}

fn origin_is_allowed(state: &AppState, origin: &str) -> bool {
    state
        .config
        .security
        .allowed_origins
        .iter()
        .any(|allowed| allowed == origin)
}

fn apply_security_headers(headers: &mut HeaderMap, include_hsts: bool) {
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; connect-src 'self'; frame-ancestors 'none'",
        ),
    );

    if include_hsts {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
}

fn apply_cors_headers(headers: &mut HeaderMap, allowed_origin: Option<&str>) {
    let Some(origin) = allowed_origin else {
        return;
    };
    let Ok(origin) = HeaderValue::from_str(origin) else {
        return;
    };

    headers.insert("access-control-allow-origin", origin);
    headers.insert(
        "access-control-allow-credentials",
        HeaderValue::from_static("true"),
    );
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static(ALLOW_METHODS),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static(ALLOW_HEADERS),
    );
    headers.insert(
        "access-control-max-age",
        HeaderValue::from_static(MAX_AGE_SECONDS),
    );
    headers.append("vary", HeaderValue::from_static("Origin"));
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        response::IntoResponse,
    };

    use super::{apply_cors_headers, apply_security_headers, is_cors_preflight};

    #[test]
    fn security_headers_include_required_baseline() {
        let mut response = StatusCode::OK.into_response();

        apply_security_headers(response.headers_mut(), true);

        assert_eq!(
            response.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        assert_eq!(response.headers().get("x-frame-options").unwrap(), "DENY");
        assert!(response.headers().contains_key("strict-transport-security"));
        assert!(response.headers().contains_key("content-security-policy"));
    }

    #[test]
    fn hsts_is_optional_for_development() {
        let mut response = StatusCode::OK.into_response();

        apply_security_headers(response.headers_mut(), false);

        assert!(!response.headers().contains_key("strict-transport-security"));
    }

    #[test]
    fn cors_never_combines_wildcard_with_credentials() {
        let mut response = StatusCode::OK.into_response();

        apply_cors_headers(response.headers_mut(), Some("https://admin.example.com"));

        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "https://admin.example.com"
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-credentials")
                .unwrap(),
            "true"
        );
        assert_ne!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap(),
            "*"
        );
    }

    #[test]
    fn preflight_requires_options_and_request_method_header() {
        let preflight = Request::builder()
            .method("OPTIONS")
            .uri("/api/admin/apps")
            .header("access-control-request-method", "POST")
            .body(Body::empty())
            .expect("preflight request");
        let plain_options = Request::builder()
            .method("OPTIONS")
            .uri("/health")
            .body(Body::empty())
            .expect("options request");

        assert!(is_cors_preflight(&preflight));
        assert!(!is_cors_preflight(&plain_options));
    }
}
