use axum::{extract::State, http::StatusCode, Extension, Json};
use serde::Serialize;

use crate::{cache, db, error::ApiResponse, http::request_id::RequestId, state::AppState};

#[derive(Serialize)]
pub struct HealthPayload {
    status: &'static str,
    service: String,
    environment: String,
}

#[derive(Serialize)]
pub struct ReadinessPayload {
    status: &'static str,
    service: String,
    environment: String,
    checks: Vec<DependencyCheck>,
}

#[derive(Serialize)]
pub struct DependencyCheck {
    name: &'static str,
    status: &'static str,
    message: Option<&'static str>,
}

pub async fn health(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> Json<ApiResponse<HealthPayload>> {
    Json(ApiResponse::ok(
        HealthPayload {
            status: "ok",
            service: state.config.app.name.clone(),
            environment: state.config.app.env.clone(),
        },
        request_id.to_string(),
    ))
}

pub async fn readiness(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
) -> (StatusCode, Json<ApiResponse<ReadinessPayload>>) {
    let database_ok = db::ping(&state.db, state.config.database.connect_timeout)
        .await
        .is_ok();
    let redis_ok = cache::ping(&state.redis, state.config.redis.connect_timeout)
        .await
        .is_ok();
    let ready = database_ok && redis_ok;

    let status_code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let payload_status = if ready { "ok" } else { "unavailable" };

    (
        status_code,
        Json(ApiResponse::ok(
            ReadinessPayload {
                status: payload_status,
                service: state.config.app.name.clone(),
                environment: state.config.app.env.clone(),
                checks: vec![
                    dependency_check("database", database_ok),
                    dependency_check("redis", redis_ok),
                ],
            },
            request_id.to_string(),
        )),
    )
}

fn dependency_check(name: &'static str, healthy: bool) -> DependencyCheck {
    DependencyCheck {
        name,
        status: if healthy { "ok" } else { "unavailable" },
        message: (!healthy).then_some("dependency check failed"),
    }
}
