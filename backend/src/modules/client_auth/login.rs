use axum::{extract::State, http::HeaderMap, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    crypto::{
        jwt::ClientAccessClaims,
        password::verify_password,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::repository::ApplicationRepository,
        audit::{self, AuditLogInput},
        client_auth::{
            access_token::prepare_client_access_token_signer,
            activate::validate_device_public_key,
            model::{ClientSession, NewClientRefreshToken, NewClientSession},
            repository::{
                create_client_refresh_token_in_transaction, create_client_session_in_transaction,
            },
            session::load_client_entitlement,
        },
        customer::repository::CustomerRepository,
        device::{
            model::{Device, DeviceBindInput, NewDeviceKey},
            repository::create_device_key_in_transaction,
            service::DeviceService,
        },
        subscription::{repository::SubscriptionRepository, service::validate_subscription_record},
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct CustomerLoginRequest {
    pub app_key: String,
    pub email: String,
    pub password: String,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os: Option<String>,
    pub app_version: Option<String>,
    pub device_public_key: String,
}

#[derive(Debug, Serialize)]
pub struct CustomerLoginResponse {
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

pub async fn login(
    State(state): State<AppState>,
    axum::Extension(request_id): axum::Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<CustomerLoginRequest>,
) -> Result<Json<ApiResponse<CustomerLoginResponse>>, AppError> {
    let email = normalize_email(&payload.email)?;
    let ip = rate_limit::client_ip(&headers);
    rate_limit::check_fixed_window(
        &state,
        rate_limit::login_key(&email, &ip),
        state.config.security.login_rate_limit_max,
        state.config.security.login_rate_limit_window_seconds,
        AppError::login_rate_limited,
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
    if application.auth_mode == "license" {
        return Err(AppError::invalid_request(
            "application does not allow subscription login",
        ));
    }

    let customer = CustomerRepository::new(state.db.clone())
        .find_by_email(application.tenant_id, &email)
        .await?
        .ok_or_else(AppError::invalid_credentials)?;
    if customer.status != "active" {
        return Err(AppError::account_disabled());
    }
    let password_hash = customer
        .password_hash
        .as_deref()
        .ok_or_else(AppError::invalid_credentials)?;
    if !verify_password(&payload.password, password_hash)? {
        return Err(AppError::invalid_credentials());
    }

    let now = Utc::now();
    let active_subscription = SubscriptionRepository::new(state.db.clone())
        .find_active_for_customer(application.tenant_id, application.id, customer.id, now)
        .await?;
    if let Some(subscription) = active_subscription.as_ref() {
        validate_subscription_record(subscription, now)?;
    }

    let access_ttl = state.config.security.client_access_token_ttl_seconds;
    let refresh_ttl = state.config.security.client_refresh_token_ttl_seconds;
    let session_ttl = state.config.security.client_session_ttl_seconds;
    let session_expires_at = now + Duration::seconds(session_ttl);
    let refresh_token = generate_token();
    let refresh_token_hash =
        hash_token(&state.config.security.refresh_token_pepper, &refresh_token)?;
    let access_token_signer = prepare_client_access_token_signer(&state, Some(&request_id)).await?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let bind_input = DeviceBindInput {
        tenant_id: application.tenant_id,
        app_id: application.id,
        customer_id: Some(customer.id),
        license_id: None,
        subscription_id: active_subscription
            .as_ref()
            .map(|subscription| subscription.id),
        machine_id: payload.machine_id,
        device_name: payload.device_name,
        os: payload.os,
        app_version: payload.app_version,
        metadata: None,
    };
    let bind_result = if let Some(subscription) = active_subscription.as_ref() {
        DeviceService::bind_for_subscription_in_transaction(
            &mut transaction,
            bind_input,
            subscription,
        )
        .await?
    } else {
        DeviceService::bind_for_customer_session_in_transaction(&mut transaction, bind_input)
            .await?
    };
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
            Some(customer.id),
            bind_result.device.id,
            bind_result.device.machine_id.clone(),
            "subscription".to_owned(),
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
            customer.id,
            device_key_id,
            session.id,
        )
        .await?;
    }
    let access_token =
        access_token_signer.sign(&access_claims(&state, &session, now, access_ttl))?;
    let entitlement = load_client_entitlement(&state, &session, &bind_result.device, now).await?;
    let active_entitlement = entitlement
        .as_ref()
        .filter(|entitlement| entitlement.active);
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CustomerLoginResponse {
            access_token,
            refresh_token,
            token_type: "Bearer",
            expires_in: access_ttl,
            refresh_expires_in: refresh_ttl,
            session_id: session.id,
            device_id: session.device_id,
            device_key_id,
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

fn normalize_email(email: &str) -> Result<String, AppError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::invalid_credentials());
    }

    Ok(email)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("client login database error: {error}"))
}

async fn audit_device_create(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    request_id: &RequestId,
    device: &Device,
    customer_id: Uuid,
    device_key_id: Option<Uuid>,
    session_id: Uuid,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(device.tenant_id),
            actor_type: "customer",
            actor_id: Some(customer_id),
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
                "auth_mode": "subscription",
                "device_key_id": device_key_id,
                "session_id": session_id,
            }),
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::normalize_email;

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email(" User@Example.COM ").expect("email"),
            "user@example.com"
        );
    }

    #[test]
    fn normalize_email_rejects_blank_or_invalid() {
        assert!(normalize_email(" ").is_err());
        assert!(normalize_email("not-email").is_err());
    }
}
