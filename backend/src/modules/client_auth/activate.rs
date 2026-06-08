use axum::{extract::State, http::HeaderMap, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    crypto::{
        jwt::ClientAccessClaims,
        signing::parse_ed25519_public_key,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::repository::ApplicationRepository,
        audit::{self, AuditLogInput},
        client_auth::{
            access_token::prepare_client_access_token_signer,
            model::{NewClientRefreshToken, NewClientSession},
            repository::{
                create_client_refresh_token_in_transaction, create_client_session_in_transaction,
            },
        },
        device::{
            model::{Device, DeviceBindInput, NewDeviceKey},
            repository::create_device_key_in_transaction,
            service::DeviceService,
        },
        license::service::LicenseService,
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct ActivateRequest {
    pub app_key: String,
    pub license_key: String,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os: Option<String>,
    pub app_version: Option<String>,
    pub device_public_key: String,
}

#[derive(Debug, Serialize)]
pub struct ActivateResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub refresh_expires_in: i64,
    pub session_id: Uuid,
    pub device_id: Uuid,
    pub device_key_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub entitlement_id: Option<Uuid>,
    pub entitlement_kind: Option<String>,
    pub entitlement_status: String,
    pub entitlement_active: bool,
    pub features: serde_json::Value,
}

pub async fn activate(
    State(state): State<AppState>,
    axum::Extension(request_id): axum::Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<ActivateRequest>,
) -> Result<Json<ApiResponse<ActivateResponse>>, AppError> {
    let ip = rate_limit::client_ip(&headers);
    rate_limit::check_fixed_window(
        &state,
        rate_limit::activation_key(&payload.app_key, &payload.machine_id, &ip),
        state.config.security.activation_rate_limit_max,
        state.config.security.activation_rate_limit_window_seconds,
        AppError::activation_rate_limited,
    )
    .await?;

    validate_device_public_key(&payload.device_public_key)?;

    let app_key = payload.app_key.trim();
    let application = ApplicationRepository::new(state.db.clone())
        .find_by_app_key(app_key)
        .await?
        .ok_or_else(|| AppError::not_found("application not found"))?;
    if application.status != "active" {
        return Err(AppError::app_disabled());
    }
    if application.auth_mode == "subscription" {
        return Err(AppError::invalid_request(
            "application does not allow license activation",
        ));
    }

    let now = Utc::now();
    let valid_license = LicenseService::new(
        state.db.clone(),
        state.config.security.token_hash_pepper.clone(),
    )
    .validate_license_key(
        application.tenant_id,
        application.id,
        &payload.license_key,
        now,
    )
    .await?;
    let access_ttl = state.config.security.client_access_token_ttl_seconds;
    let refresh_ttl = state.config.security.client_refresh_token_ttl_seconds;
    let session_ttl = state.config.security.client_session_ttl_seconds;
    let session_expires_at = now + Duration::seconds(session_ttl);
    let refresh_token = generate_token();
    let refresh_token_hash =
        hash_token(&state.config.security.refresh_token_pepper, &refresh_token)?;
    let access_token_signer = prepare_client_access_token_signer(&state, Some(&request_id)).await?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let bind_result = DeviceService::bind_for_license_in_transaction(
        &mut transaction,
        DeviceBindInput {
            tenant_id: application.tenant_id,
            app_id: application.id,
            customer_id: valid_license.license.customer_id,
            license_id: Some(valid_license.license.id),
            subscription_id: None,
            machine_id: payload.machine_id,
            device_name: payload.device_name,
            os: payload.os,
            app_version: payload.app_version,
            metadata: None,
        },
        &valid_license.license,
    )
    .await?;
    let device_key_id = if bind_result.created {
        let device_key = create_device_key_in_transaction(
            &mut transaction,
            NewDeviceKey::new(
                application.tenant_id,
                application.id,
                bind_result.device.id,
                payload.device_public_key,
            )?,
        )
        .await?;
        Some(device_key.id)
    } else {
        None
    };
    let session = create_client_session_in_transaction(
        &mut transaction,
        NewClientSession::new(
            application.tenant_id,
            application.id,
            valid_license.license.customer_id,
            bind_result.device.id,
            bind_result.device.machine_id.clone(),
            "license".to_owned(),
            session_expires_at,
        ),
    )
    .await?;
    create_client_refresh_token_in_transaction(
        &mut transaction,
        NewClientRefreshToken::new(
            session.id,
            refresh_token_hash,
            now + Duration::seconds(refresh_ttl),
        ),
    )
    .await?;
    if bind_result.created {
        audit_device_create(
            &mut transaction,
            &request_id,
            &bind_result.device,
            "license",
            device_key_id,
            session.id,
        )
        .await?;
    }
    let access_token = access_token_signer.sign(&ClientAccessClaims {
        sub: session.id.to_string(),
        iss: state.config.security.jwt_issuer.clone(),
        aud: state.config.security.jwt_audience.clone(),
        exp: (now + Duration::seconds(access_ttl)).timestamp(),
        iat: now.timestamp(),
        session_id: session.id,
        tenant_id: session.tenant_id,
        app_id: session.app_id,
        device_id: session.device_id,
        machine_id: session.machine_id,
        auth_mode: session.auth_mode,
    })?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ActivateResponse {
            access_token,
            refresh_token,
            token_type: "Bearer",
            expires_in: access_ttl,
            refresh_expires_in: refresh_ttl,
            session_id: session.id,
            device_id: bind_result.device.id,
            device_key_id,
            subscription_id: None,
            entitlement_id: Some(valid_license.license.id),
            entitlement_kind: Some("license".to_owned()),
            entitlement_status: valid_license.license.status,
            entitlement_active: true,
            features: valid_license.license.features,
        },
        request_id.to_string(),
    )))
}

pub fn validate_device_public_key(public_key: &str) -> Result<(), AppError> {
    let public_key = public_key.trim();
    if public_key.is_empty() {
        return Err(AppError::validation_failed("device_public_key is required"));
    }
    parse_ed25519_public_key(public_key)
        .map_err(|_| AppError::validation_failed("device_public_key invalid"))?;

    Ok(())
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("client activation database error: {error}"))
}

async fn audit_device_create(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_id: &RequestId,
    device: &Device,
    auth_mode: &'static str,
    device_key_id: Option<Uuid>,
    session_id: Uuid,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(device.tenant_id),
            actor_type: "device",
            actor_id: Some(device.id),
            action: "device.create",
            resource_type: "device",
            resource_id: Some(device.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(serde_json::json!({
                "id": device.id,
                "app_id": device.app_id,
                "customer_id": device.customer_id,
                "license_id": device.license_id,
                "subscription_id": device.subscription_id,
                "machine_id": device.machine_id,
                "device_name": device.device_name,
                "os": device.os,
                "app_version": device.app_version,
                "status": device.status,
            })),
            metadata_json: serde_json::json!({
                "auth_mode": auth_mode,
                "device_key_id": device_key_id,
                "session_id": session_id,
            }),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use crate::crypto::signing::generate_ed25519_key;

    use super::validate_device_public_key;

    #[test]
    fn device_public_key_must_be_valid_ed25519_key() {
        let key = generate_ed25519_key().expect("key should generate");

        assert!(validate_device_public_key(&key.public_key_pem).is_ok());
        assert!(validate_device_public_key("public-key").is_err());
        assert!(validate_device_public_key(" ").is_err());
    }
}
