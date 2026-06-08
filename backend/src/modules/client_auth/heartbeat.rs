use axum::{extract::State, Extension, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        client_auth::{repository::ClientAuthRepository, session::ClientContext},
        device::repository::DeviceRepository,
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct HeartbeatRequest {
    pub app_version: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HeartbeatResponse {
    pub status: &'static str,
    pub server_time: i64,
    pub license_status: String,
    pub entitlement_id: Option<Uuid>,
    pub entitlement_kind: Option<String>,
    pub entitlement_status: String,
    pub entitlement_active: bool,
    pub subscription_id: Option<Uuid>,
}

pub async fn heartbeat(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<HeartbeatRequest>,
) -> Result<Json<ApiResponse<HeartbeatResponse>>, AppError> {
    rate_limit::check_fixed_window(
        &state,
        rate_limit::heartbeat_key(&client.device_id.to_string()),
        state.config.security.heartbeat_rate_limit_max,
        state.config.security.heartbeat_rate_limit_window_seconds,
        AppError::rate_limited,
    )
    .await?;

    ClientAuthRepository::new(state.db.clone())
        .touch_session(client.session_id)
        .await?;
    DeviceRepository::new(state.db.clone())
        .touch_device(
            client.tenant_id,
            client.app_id,
            client.device_id,
            clean_optional(payload.app_version),
        )
        .await?;

    Ok(Json(ApiResponse::ok(
        HeartbeatResponse {
            status: "ok",
            server_time: Utc::now().timestamp(),
            license_status: client.entitlement_status.clone(),
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

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use super::clean_optional;

    #[test]
    fn clean_optional_turns_blank_into_none() {
        assert_eq!(clean_optional(Some(" ".to_owned())), None);
        assert_eq!(
            clean_optional(Some("1.0.0".to_owned())),
            Some("1.0.0".to_owned())
        );
    }
}
