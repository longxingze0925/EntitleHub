use axum::{extract::State, Extension, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::client_auth::session::ClientContext,
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub valid: bool,
    pub features: serde_json::Value,
    pub expires_at: Option<DateTime<Utc>>,
}

pub async fn verify(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<VerifyResponse>>, AppError> {
    rate_limit::check_client_action(&state, "verify", &client.device_id.to_string()).await?;

    Ok(Json(ApiResponse::ok(
        VerifyResponse {
            valid: true,
            features: client.features,
            expires_at: client.entitlement_expires_at,
        },
        request_id.to_string(),
    )))
}
