use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::{
        application::repository::ApplicationRepository,
        client_auth::{
            access_token::verify_client_access_token, model::ClientSession,
            repository::ClientAuthRepository,
        },
        device::{model::Device, repository::DeviceRepository},
        license::{repository::LicenseRepository, service::validate_license_record},
        subscription::{repository::SubscriptionRepository, service::validate_subscription_record},
        tenant::repository::TenantRepository,
    },
    state::AppState,
};

#[derive(Debug, Clone)]
pub struct ClientContext {
    pub session_id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub device_id: Uuid,
    pub machine_id: String,
    pub auth_mode: String,
    pub entitlement_id: Uuid,
    pub entitlement_kind: String,
    pub entitlement_status: String,
    pub features: Value,
    pub entitlement_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ClientEntitlement {
    pub id: Uuid,
    pub kind: &'static str,
    pub status: String,
    pub features: Value,
    pub expires_at: Option<DateTime<Utc>>,
}

pub async fn require_client_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let token = bearer_token(request.headers())?;
    let now = Utc::now();
    let claims = verify_client_access_token(&state, token).await?;
    let repository = ClientAuthRepository::new(state.db.clone());
    let session = repository
        .find_session_by_id(claims.session_id)
        .await?
        .ok_or_else(AppError::session_expired)?;

    if session.revoked_at.is_some() || session.expires_at <= now {
        return Err(AppError::session_expired());
    }

    if session.tenant_id != claims.tenant_id
        || session.app_id != claims.app_id
        || session.device_id != claims.device_id
        || session.machine_id != claims.machine_id
    {
        return Err(AppError::token_invalid("access token invalid"));
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

    request.extensions_mut().insert(ClientContext {
        session_id: session.id,
        tenant_id: session.tenant_id,
        app_id: session.app_id,
        customer_id: session.customer_id,
        device_id: session.device_id,
        machine_id: session.machine_id,
        auth_mode: session.auth_mode,
        entitlement_id: entitlement.id,
        entitlement_kind: entitlement.kind.to_owned(),
        entitlement_status: entitlement.status,
        features: entitlement.features,
        entitlement_expires_at: entitlement.expires_at,
    });

    Ok(next.run(request).await)
}

pub async fn ensure_active_tenant(state: &AppState, tenant_id: Uuid) -> Result<(), AppError> {
    let tenant = TenantRepository::new(state.db.clone())
        .find_by_id(tenant_id)
        .await?
        .ok_or_else(AppError::session_expired)?;

    if tenant.status != "active" {
        return Err(AppError::tenant_forbidden());
    }

    Ok(())
}

pub async fn load_client_entitlement(
    state: &AppState,
    session: &ClientSession,
    device: &Device,
    now: DateTime<Utc>,
) -> Result<ClientEntitlement, AppError> {
    if let Some(license_id) = device.license_id {
        let license = LicenseRepository::new(state.db.clone())
            .find_by_id(session.tenant_id, license_id)
            .await?
            .ok_or_else(AppError::license_not_found)?;
        validate_license_record(&license, now)?;

        return Ok(ClientEntitlement {
            id: license.id,
            kind: "license",
            status: license.status,
            features: license.features,
            expires_at: license.expires_at,
        });
    }

    if let Some(subscription_id) = device.subscription_id {
        let subscription = SubscriptionRepository::new(state.db.clone())
            .find_by_id(session.tenant_id, subscription_id)
            .await?
            .ok_or_else(|| AppError::license_invalid("subscription not found"))?;
        validate_subscription_record(&subscription, now)?;

        return Ok(ClientEntitlement {
            id: subscription.id,
            kind: "subscription",
            status: subscription.status,
            features: subscription.features,
            expires_at: subscription.expires_at,
        });
    }

    Err(AppError::license_invalid("session has no entitlement"))
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Result<&str, AppError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(AppError::unauthenticated)?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(AppError::unauthenticated());
    };
    if token.trim().is_empty() {
        return Err(AppError::unauthenticated());
    }

    Ok(token)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::bearer_token;

    #[test]
    fn bearer_token_reads_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer token"),
        );

        assert_eq!(bearer_token(&headers).expect("bearer"), "token");
    }

    #[test]
    fn bearer_token_rejects_missing_prefix() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("token"),
        );

        assert!(bearer_token(&headers).is_err());
    }
}
