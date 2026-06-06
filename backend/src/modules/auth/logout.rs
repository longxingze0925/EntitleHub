use axum::{
    extract::State,
    http::header::SET_COOKIE,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Serialize;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::auth::{
        admin_session::AdminSessionRepository,
        csrf::build_clear_csrf_cookie,
        session::{build_clear_refresh_cookie, build_clear_session_cookie, AdminContext},
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct LogoutResponse {}

pub async fn logout(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Response, AppError> {
    let repository = AdminSessionRepository::new(state.db.clone());
    repository.revoke(admin.session_id).await?;
    repository
        .revoke_refresh_tokens_for_session(admin.session_id)
        .await?;

    let body = ApiResponse::ok(LogoutResponse {}, request_id.to_string());
    let mut response = Json(body).into_response();
    let cookie = build_clear_session_cookie(state.config.security.cookie_secure)?;
    let refresh_cookie = build_clear_refresh_cookie(state.config.security.cookie_secure)?;
    let csrf_cookie = build_clear_csrf_cookie(state.config.security.cookie_secure)?;
    response.headers_mut().insert(SET_COOKIE, cookie);
    response.headers_mut().append(SET_COOKIE, refresh_cookie);
    response.headers_mut().append(SET_COOKIE, csrf_cookie);

    Ok(response)
}
