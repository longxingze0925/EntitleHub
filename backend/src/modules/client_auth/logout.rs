use axum::{extract::State, Extension, Json};
use serde::Serialize;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::client_auth::{repository::ClientAuthRepository, session::ClientContext},
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub revoked: bool,
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<LogoutResponse>>, AppError> {
    rate_limit::check_client_action(&state, "logout", &client.device_id.to_string()).await?;

    let revoked = ClientAuthRepository::new(state.db.clone())
        .revoke_session(client.session_id)
        .await?
        > 0;

    Ok(Json(ApiResponse::ok(
        LogoutResponse { revoked },
        request_id.to_string(),
    )))
}
