use axum::{extract::State, http::HeaderMap, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        client_auth::{activate::validate_device_public_key, session::ClientContext},
        device::{
            model::{DeviceKey, DeviceSummary, NewDeviceKey},
            repository::{
                create_device_key_in_transaction, revoke_device_refresh_tokens_in_transaction,
                revoke_device_sessions_in_transaction, rotate_device_key_in_transaction,
                set_device_status_in_transaction, DeviceRepository,
            },
        },
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct SelfUnbindDeviceResponse {
    pub device: DeviceSummary,
    pub revoked_sessions: u64,
}

#[derive(Debug, Deserialize)]
pub struct RotateSelfDeviceKeyRequest {
    pub device_public_key: String,
}

#[derive(Debug, Serialize)]
pub struct RotateSelfDeviceKeyResponse {
    pub device_key_id: Uuid,
    pub device_public_key: String,
    pub algorithm: String,
    pub status: String,
    pub rotated_device_key_ids: Vec<Uuid>,
}

pub async fn unbind_self(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<SelfUnbindDeviceResponse>>, AppError> {
    rate_limit::check_client_action(&state, "self_unbind", &client.device_id.to_string()).await?;

    let repository = DeviceRepository::new(state.db.clone());
    let before = repository
        .find_by_id(client.tenant_id, client.app_id, client.device_id)
        .await?
        .ok_or_else(AppError::device_not_found)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let device = set_device_status_in_transaction(
        &mut transaction,
        client.tenant_id,
        client.device_id,
        "unbound",
    )
    .await?
    .ok_or_else(AppError::device_not_found)?;
    let revoked_refresh_tokens = revoke_device_refresh_tokens_in_transaction(
        &mut transaction,
        client.tenant_id,
        client.device_id,
    )
    .await?;
    let revoked_sessions =
        revoke_device_sessions_in_transaction(&mut transaction, client.tenant_id, client.device_id)
            .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(client.tenant_id),
            actor_type: if client.customer_id.is_some() {
                "customer"
            } else {
                "device"
            },
            actor_id: client.customer_id.or(Some(client.device_id)),
            action: "device.self_unbind",
            resource_type: "device",
            resource_id: Some(client.device_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(device_audit_json(&before)),
            after_json: Some(device_audit_json(&device)),
            metadata_json: json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
                "entitlement_kind": client.entitlement_kind,
                "entitlement_id": client.entitlement_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        SelfUnbindDeviceResponse {
            device: device.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn rotate_self_key(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<RotateSelfDeviceKeyRequest>,
) -> Result<Json<ApiResponse<RotateSelfDeviceKeyResponse>>, AppError> {
    rate_limit::check_client_action(&state, "self_key_rotate", &client.device_id.to_string())
        .await?;
    validate_device_public_key(&payload.device_public_key)?;
    let current_device_key_id = current_device_key_id(&headers)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let rotated_device_key = rotate_device_key_in_transaction(
        &mut transaction,
        client.tenant_id,
        client.app_id,
        client.device_id,
        current_device_key_id,
    )
    .await?
    .ok_or_else(|| AppError::conflict("active device key changed"))?;
    let device_key = create_device_key_in_transaction(
        &mut transaction,
        NewDeviceKey::new(
            client.tenant_id,
            client.app_id,
            client.device_id,
            payload.device_public_key,
        )?,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(client.tenant_id),
            actor_type: if client.customer_id.is_some() {
                "customer"
            } else {
                "device"
            },
            actor_id: client.customer_id.or(Some(client.device_id)),
            action: "device.self_key_rotate",
            resource_type: "device_key",
            resource_id: Some(device_key.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(device_key_audit_json(&rotated_device_key)),
            after_json: Some(device_key_audit_json(&device_key)),
            metadata_json: json!({
                "device_id": client.device_id,
                "previous_device_key_id": rotated_device_key.id,
                "entitlement_kind": client.entitlement_kind,
                "entitlement_id": client.entitlement_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RotateSelfDeviceKeyResponse {
            device_key_id: device_key.id,
            device_public_key: device_key.public_key,
            algorithm: device_key.algorithm,
            status: device_key.status,
            rotated_device_key_ids: vec![rotated_device_key.id],
        },
        request_id.to_string(),
    )))
}

fn device_audit_json(device: &crate::modules::device::model::Device) -> serde_json::Value {
    json!({
        "id": device.id,
        "app_id": device.app_id,
        "customer_id": device.customer_id,
        "license_id": device.license_id,
        "subscription_id": device.subscription_id,
        "machine_id": device.machine_id,
        "status": device.status,
    })
}

fn device_key_audit_json(device_key: &DeviceKey) -> serde_json::Value {
    json!({
        "id": device_key.id,
        "app_id": device_key.app_id,
        "device_id": device_key.device_id,
        "algorithm": device_key.algorithm,
        "status": device_key.status,
        "created_at": device_key.created_at,
        "rotated_at": device_key.rotated_at,
        "revoked_at": device_key.revoked_at,
    })
}

fn current_device_key_id(headers: &HeaderMap) -> Result<Uuid, AppError> {
    headers
        .get("X-Device-Key-Id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::signature_required("X-Device-Key-Id is required"))?
        .parse()
        .map_err(|_| AppError::signature_invalid("X-Device-Key-Id is invalid"))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("client device database error: {error}"))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use crate::modules::device::model::DeviceKey;

    use super::{current_device_key_id, device_key_audit_json, RotateSelfDeviceKeyResponse};

    #[test]
    fn current_device_key_id_reads_verified_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Device-Key-Id",
            "00000000-0000-0000-0000-000000000001"
                .parse()
                .expect("header"),
        );

        let key_id = current_device_key_id(&headers).expect("device key id");

        assert_eq!(key_id.to_string(), "00000000-0000-0000-0000-000000000001");
    }

    #[test]
    fn current_device_key_id_rejects_missing_or_invalid_header() {
        assert!(current_device_key_id(&HeaderMap::new()).is_err());

        let mut headers = HeaderMap::new();
        headers.insert("X-Device-Key-Id", "bad".parse().expect("header"));

        assert!(current_device_key_id(&headers).is_err());
    }

    #[test]
    fn rotate_self_key_response_contains_new_key_and_rotated_ids() {
        let new_key_id = Uuid::parse_str("00000000-0000-0000-0000-000000000002").expect("uuid");
        let old_key_id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("uuid");

        let value = serde_json::to_value(RotateSelfDeviceKeyResponse {
            device_key_id: new_key_id,
            device_public_key: "public-key-pem".to_owned(),
            algorithm: "Ed25519".to_owned(),
            status: "active".to_owned(),
            rotated_device_key_ids: vec![old_key_id],
        })
        .expect("response should serialize");

        assert_eq!(
            value,
            json!({
                "device_key_id": new_key_id,
                "device_public_key": "public-key-pem",
                "algorithm": "Ed25519",
                "status": "active",
                "rotated_device_key_ids": [old_key_id],
            })
        );
    }

    #[test]
    fn device_key_audit_json_excludes_public_key_material() {
        let device_key = DeviceKey {
            id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("uuid"),
            tenant_id: Uuid::parse_str("00000000-0000-0000-0000-000000000010").expect("uuid"),
            app_id: Uuid::parse_str("00000000-0000-0000-0000-000000000011").expect("uuid"),
            device_id: Uuid::parse_str("00000000-0000-0000-0000-000000000012").expect("uuid"),
            public_key: "public-key-pem".to_owned(),
            algorithm: "Ed25519".to_owned(),
            status: "rotated".to_owned(),
            created_at: Utc::now(),
            rotated_at: Some(Utc::now()),
            revoked_at: None,
        };

        let value = device_key_audit_json(&device_key);
        let object = value.as_object().expect("audit json should be object");

        assert_eq!(object.get("id"), Some(&json!(device_key.id)));
        assert_eq!(object.get("status"), Some(&json!("rotated")));
        assert!(!object.contains_key("tenant_id"));
        assert!(!object.contains_key("public_key"));
    }
}
