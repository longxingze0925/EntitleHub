use axum::{
    extract::{Request, State},
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
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
    pub entitlement_id: Option<Uuid>,
    pub entitlement_kind: Option<String>,
    pub entitlement_status: String,
    pub entitlement_active: bool,
    pub features: Value,
    pub entitlement_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct ClientEntitlement {
    pub id: Uuid,
    pub kind: &'static str,
    pub status: String,
    pub active: bool,
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
    let active_entitlement = entitlement
        .as_ref()
        .filter(|entitlement| entitlement.active);

    request.extensions_mut().insert(ClientContext {
        session_id: session.id,
        tenant_id: session.tenant_id,
        app_id: session.app_id,
        customer_id: session.customer_id,
        device_id: session.device_id,
        machine_id: session.machine_id,
        auth_mode: session.auth_mode,
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
            .unwrap_or_else(|| json!([])),
        entitlement_expires_at: entitlement
            .as_ref()
            .and_then(|entitlement| entitlement.expires_at),
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
) -> Result<Option<ClientEntitlement>, AppError> {
    if let Some(license_id) = device.license_id {
        let license = LicenseRepository::new(state.db.clone())
            .find_by_id(session.tenant_id, license_id)
            .await?
            .ok_or_else(AppError::license_not_found)?;
        let active = validate_license_record(&license, now).is_ok();

        return Ok(Some(ClientEntitlement {
            id: license.id,
            kind: "license",
            status: license.status,
            active,
            features: license.features,
            expires_at: license.expires_at,
        }));
    }

    if let Some(subscription_id) = device.subscription_id {
        let subscription = SubscriptionRepository::new(state.db.clone())
            .find_by_id(session.tenant_id, subscription_id)
            .await?
            .ok_or_else(|| AppError::license_invalid("subscription not found"))?;
        let active = validate_subscription_record(&subscription, now).is_ok();

        return Ok(Some(ClientEntitlement {
            id: subscription.id,
            kind: "subscription",
            status: subscription.status,
            active,
            features: subscription.features,
            expires_at: subscription.expires_at,
        }));
    }

    Ok(None)
}

pub fn ensure_active_entitlement(client: &ClientContext) -> Result<(), AppError> {
    if client.entitlement_active && client.entitlement_id.is_some() {
        return Ok(());
    }

    Err(AppError::subscription_inactive(
        "active subscription or license required",
    ))
}

pub fn ensure_active_subscription(client: &ClientContext) -> Result<(), AppError> {
    if client.entitlement_active && client.entitlement_kind.as_deref() == Some("subscription") {
        return Ok(());
    }

    Err(AppError::subscription_inactive(
        "active subscription required",
    ))
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
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        bearer_token, ensure_active_entitlement, ensure_active_subscription, ClientContext,
    };

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

    #[test]
    fn inactive_or_missing_entitlement_rejects_feature_access() {
        let client = fixture_client_context(None, false);

        assert!(ensure_active_entitlement(&client).is_err());
        assert!(ensure_active_subscription(&client).is_err());
    }

    #[test]
    fn active_license_allows_entitlement_features_but_not_ai_subscription() {
        let client = fixture_client_context(Some("license"), true);

        assert!(ensure_active_entitlement(&client).is_ok());
        assert!(ensure_active_subscription(&client).is_err());
    }

    #[test]
    fn active_subscription_allows_ai_access() {
        let client = fixture_client_context(Some("subscription"), true);

        assert!(ensure_active_entitlement(&client).is_ok());
        assert!(ensure_active_subscription(&client).is_ok());
    }

    fn fixture_client_context(kind: Option<&str>, active: bool) -> ClientContext {
        ClientContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: Some(Uuid::nil()),
            device_id: Uuid::nil(),
            machine_id: "machine".to_owned(),
            auth_mode: "subscription".to_owned(),
            entitlement_id: kind.map(|_| Uuid::nil()),
            entitlement_kind: kind.map(str::to_owned),
            entitlement_status: kind
                .map(|_| "active".to_owned())
                .unwrap_or_else(|| "none".to_owned()),
            entitlement_active: active,
            features: json!([]),
            entitlement_expires_at: None,
        }
    }
}
