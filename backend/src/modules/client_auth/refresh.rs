use axum::{extract::State, http::HeaderMap, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    crypto::{
        jwt::ClientAccessClaims,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::repository::ApplicationRepository,
        client_auth::{
            access_token::prepare_client_access_token_signer,
            model::{ClientSession, NewClientRefreshToken},
            repository::{
                create_client_refresh_token_in_transaction, mark_refresh_token_used_in_transaction,
                ClientAuthRepository,
            },
            session::{ensure_active_tenant, load_client_entitlement},
        },
        device::repository::DeviceRepository,
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub refresh_expires_in: i64,
    pub session_id: Uuid,
    pub device_id: Uuid,
    pub subscription_id: Option<Uuid>,
    pub entitlement_id: Option<Uuid>,
    pub entitlement_kind: Option<String>,
    pub entitlement_status: String,
    pub entitlement_active: bool,
    pub features: serde_json::Value,
}

pub async fn refresh(
    State(state): State<AppState>,
    axum::Extension(request_id): axum::Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<RefreshRequest>,
) -> Result<Json<ApiResponse<RefreshResponse>>, AppError> {
    let refresh_token = payload.refresh_token.trim();
    if refresh_token.is_empty() {
        return Err(AppError::invalid_request("refresh_token is required"));
    }

    let now = Utc::now();
    let ip = rate_limit::client_ip(&headers);
    let token_hash = hash_token(&state.config.security.refresh_token_pepper, refresh_token)?;
    let repository = ClientAuthRepository::new(state.db.clone());
    let Some(stored_token) = repository.find_refresh_token_by_hash(&token_hash).await? else {
        rate_limit::check_fixed_window(
            &state,
            rate_limit::refresh_key("invalid-token", &ip),
            state.config.security.refresh_rate_limit_max,
            state.config.security.refresh_rate_limit_window_seconds,
            AppError::refresh_rate_limited,
        )
        .await?;
        return Err(AppError::token_invalid("refresh token invalid"));
    };
    rate_limit::check_fixed_window(
        &state,
        rate_limit::refresh_key(&stored_token.session_id.to_string(), &ip),
        state.config.security.refresh_rate_limit_max,
        state.config.security.refresh_rate_limit_window_seconds,
        AppError::refresh_rate_limited,
    )
    .await?;

    let session = repository
        .find_session_by_id(stored_token.session_id)
        .await?
        .ok_or_else(AppError::session_expired)?;

    if stored_token.used_at.is_some() || stored_token.revoked_at.is_some() {
        repository.revoke_session(session.id).await?;
        return Err(AppError::refresh_reuse_detected());
    }

    if stored_token.expires_at <= now || session.expires_at <= now || session.revoked_at.is_some() {
        return Err(AppError::session_expired());
    }
    ensure_active_tenant(&state, session.tenant_id).await?;

    let application = ApplicationRepository::new(state.db.clone())
        .find_by_id(session.tenant_id, session.app_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;
    if application.status != "active" {
        return Err(AppError::app_disabled());
    }

    let device = DeviceRepository::new(state.db.clone())
        .find_by_id(session.tenant_id, session.app_id, session.device_id)
        .await?
        .ok_or_else(AppError::device_not_found)?;
    if device.status == "blacklisted" {
        return Err(AppError::device_blacklisted());
    }
    if device.status != "active" {
        return Err(AppError::device_not_activated());
    }

    let entitlement = load_client_entitlement(&state, &session, &device, now).await?;
    let active_entitlement = entitlement
        .as_ref()
        .filter(|entitlement| entitlement.active);

    let next_refresh_token = generate_token();
    let next_refresh_token_hash = hash_token(
        &state.config.security.refresh_token_pepper,
        &next_refresh_token,
    )?;
    let refresh_ttl = state.config.security.client_refresh_token_ttl_seconds;
    let access_token_signer = prepare_client_access_token_signer(&state, Some(&request_id)).await?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    if let Err(error) =
        mark_refresh_token_used_in_transaction(&mut transaction, stored_token.id).await
    {
        let reuse_detected = matches!(&error, AppError::RefreshReuseDetected(_));
        transaction.rollback().await.map_err(map_db_error)?;
        if reuse_detected {
            repository.revoke_session(session.id).await?;
        }
        return Err(error);
    }
    create_client_refresh_token_in_transaction(
        &mut transaction,
        NewClientRefreshToken::new(
            session.id,
            next_refresh_token_hash,
            now + Duration::seconds(refresh_ttl),
        ),
    )
    .await?;

    let access_ttl = state.config.security.client_access_token_ttl_seconds;
    let access_token =
        access_token_signer.sign(&access_claims(&state, &session, now, access_ttl))?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RefreshResponse {
            access_token,
            refresh_token: next_refresh_token,
            token_type: "Bearer",
            expires_in: access_ttl,
            refresh_expires_in: refresh_ttl,
            session_id: session.id,
            device_id: session.device_id,
            subscription_id: entitlement
                .as_ref()
                .filter(|entitlement| entitlement.kind == "subscription")
                .map(|entitlement| entitlement.id),
            entitlement_id: entitlement.as_ref().map(|entitlement| entitlement.id),
            entitlement_kind: entitlement
                .as_ref()
                .map(|entitlement| entitlement.kind.to_owned()),
            entitlement_status: entitlement
                .as_ref()
                .map(|entitlement| entitlement.status.clone())
                .unwrap_or_else(|| "none".to_owned()),
            entitlement_active: active_entitlement.is_some(),
            features: active_entitlement
                .map(|entitlement| entitlement.features.clone())
                .unwrap_or_else(|| serde_json::json!([])),
        },
        request_id.to_string(),
    )))
}

fn access_claims(
    state: &AppState,
    session: &ClientSession,
    now: chrono::DateTime<Utc>,
    access_ttl: i64,
) -> ClientAccessClaims {
    ClientAccessClaims {
        sub: session.id.to_string(),
        iss: state.config.security.jwt_issuer.clone(),
        aud: state.config.security.jwt_audience.clone(),
        exp: (now + Duration::seconds(access_ttl)).timestamp(),
        iat: now.timestamp(),
        session_id: session.id,
        tenant_id: session.tenant_id,
        app_id: session.app_id,
        device_id: session.device_id,
        machine_id: session.machine_id.clone(),
        auth_mode: session.auth_mode.clone(),
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("client refresh database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::RefreshRequest;

    #[test]
    fn refresh_request_deserializes() {
        let request: RefreshRequest =
            serde_json::from_value(serde_json::json!({"refresh_token": "token"}))
                .expect("request should deserialize");

        assert_eq!(request.refresh_token, "token");
    }
}
