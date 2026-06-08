use axum::{extract::State, Extension, Json};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

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
    pub entitlement_id: Option<Uuid>,
    pub entitlement_kind: Option<String>,
    pub entitlement_status: String,
    pub entitlement_active: bool,
    pub subscription_id: Option<Uuid>,
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
            features: client.features.clone(),
            expires_at: client.entitlement_expires_at,
            entitlement_id: client.entitlement_id,
            entitlement_kind: client.entitlement_kind.clone(),
            entitlement_status: client.entitlement_status.clone(),
            entitlement_active: client.entitlement_active,
            subscription_id: (client.entitlement_kind.as_deref() == Some("subscription"))
                .then_some(client.entitlement_id)
                .flatten(),
        },
        request_id.to_string(),
    )))
}
