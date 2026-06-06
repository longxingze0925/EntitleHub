use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        device::{
            model::{
                validate_device_status_filter, Device, DeviceListMeta, DeviceListQuery,
                DeviceSummary,
            },
            repository::{
                add_device_blacklist_in_transaction, remove_device_blacklist_in_transaction,
                revoke_device_refresh_tokens_in_transaction, revoke_device_sessions_in_transaction,
                set_device_status_in_transaction, DeviceRepository,
            },
        },
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct DeviceListResponse {
    pub items: Vec<DeviceSummary>,
    pub meta: DeviceListMeta,
}

#[derive(Debug, Serialize)]
pub struct DeviceDetailResponse {
    pub device: DeviceSummary,
}

#[derive(Debug, Deserialize)]
pub struct BlacklistDeviceInput {
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DeviceMutationResponse {
    pub device: DeviceSummary,
    pub revoked_sessions: u64,
}

pub async fn list_devices(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<DeviceListQuery>,
) -> Result<Json<ApiResponse<DeviceListResponse>>, AppError> {
    ensure_admin_permission(&admin, "device:read")?;
    validate_device_status_filter(query.status.as_deref())?;

    let devices = DeviceRepository::new(state.db.clone())
        .list(admin.tenant_id, &query)
        .await?;
    let items = devices.into_iter().map(DeviceSummary::from).collect();

    Ok(Json(ApiResponse::ok(
        DeviceListResponse {
            items,
            meta: DeviceListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn get_device(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(device_id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeviceDetailResponse>>, AppError> {
    ensure_admin_permission(&admin, "device:read")?;

    let device = DeviceRepository::new(state.db.clone())
        .find_by_id_for_admin(admin.tenant_id, device_id)
        .await?
        .ok_or_else(AppError::device_not_found)?;

    Ok(Json(ApiResponse::ok(
        DeviceDetailResponse {
            device: device.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn unbind_device(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(device_id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeviceMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "device:unbind")?;

    let repository = DeviceRepository::new(state.db.clone());
    let before = repository
        .find_by_id_for_admin(admin.tenant_id, device_id)
        .await?
        .ok_or_else(AppError::device_not_found)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let device =
        set_device_status_in_transaction(&mut transaction, admin.tenant_id, device_id, "unbound")
            .await?
            .ok_or_else(AppError::device_not_found)?;
    let revoked_refresh_tokens =
        revoke_device_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id, device_id)
            .await?;
    let revoked_sessions =
        revoke_device_sessions_in_transaction(&mut transaction, admin.tenant_id, device_id).await?;
    audit_device_change(
        &mut transaction,
        &admin,
        &request_id,
        "device.unbind",
        &before,
        &device,
        json!({
            "revoked_sessions": revoked_sessions,
            "revoked_refresh_tokens": revoked_refresh_tokens,
        }),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeviceMutationResponse {
            device: device.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn blacklist_device(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(device_id): Path<Uuid>,
    Json(payload): Json<BlacklistDeviceInput>,
) -> Result<Json<ApiResponse<DeviceMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "device:blacklist")?;

    let reason = normalize_blacklist_reason(payload.reason)?;
    let repository = DeviceRepository::new(state.db.clone());
    let before = repository
        .find_by_id_for_admin(admin.tenant_id, device_id)
        .await?
        .ok_or_else(AppError::device_not_found)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    add_device_blacklist_in_transaction(
        &mut transaction,
        admin.tenant_id,
        before.app_id,
        &before.machine_id,
        &reason,
        admin.team_member_id,
    )
    .await?;
    let device = set_device_status_in_transaction(
        &mut transaction,
        admin.tenant_id,
        device_id,
        "blacklisted",
    )
    .await?
    .ok_or_else(AppError::device_not_found)?;
    let revoked_refresh_tokens =
        revoke_device_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id, device_id)
            .await?;
    let revoked_sessions =
        revoke_device_sessions_in_transaction(&mut transaction, admin.tenant_id, device_id).await?;
    audit_device_change(
        &mut transaction,
        &admin,
        &request_id,
        "device.blacklist",
        &before,
        &device,
        json!({
            "reason": reason,
            "revoked_sessions": revoked_sessions,
            "revoked_refresh_tokens": revoked_refresh_tokens,
        }),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeviceMutationResponse {
            device: device.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn unblacklist_device(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(device_id): Path<Uuid>,
) -> Result<Json<ApiResponse<DeviceMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "device:unblacklist")?;

    let repository = DeviceRepository::new(state.db.clone());
    let before = repository
        .find_by_id_for_admin(admin.tenant_id, device_id)
        .await?
        .ok_or_else(AppError::device_not_found)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let removed_blacklist = remove_device_blacklist_in_transaction(
        &mut transaction,
        admin.tenant_id,
        before.app_id,
        &before.machine_id,
    )
    .await?;
    let device =
        set_device_status_in_transaction(&mut transaction, admin.tenant_id, device_id, "active")
            .await?
            .ok_or_else(AppError::device_not_found)?;
    audit_device_change(
        &mut transaction,
        &admin,
        &request_id,
        "device.unblacklist",
        &before,
        &device,
        json!({ "removed_blacklist": removed_blacklist }),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeviceMutationResponse {
            device: device.into(),
            revoked_sessions: 0,
        },
        request_id.to_string(),
    )))
}

async fn audit_device_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: &Device,
    device: &Device,
    metadata: Value,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "device",
            resource_id: Some(device.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(device_audit_json(before)),
            after_json: Some(device_audit_json(device)),
            metadata_json: metadata,
        },
    )
    .await
}

fn device_audit_json(device: &Device) -> Value {
    json!({
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
        "last_seen_at": device.last_seen_at,
    })
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn normalize_blacklist_reason(reason: Option<String>) -> Result<String, AppError> {
    let reason =
        clean_optional(reason).ok_or_else(|| AppError::validation_failed("reason is required"))?;
    if reason.chars().count() > 500 {
        return Err(AppError::validation_failed("reason is too long"));
    }

    Ok(reason)
}

fn ensure_admin_permission(admin: &AdminContext, permission_code: &str) -> Result<(), AppError> {
    if admin
        .permissions
        .iter()
        .any(|permission| permission == permission_code)
    {
        return Ok(());
    }

    Err(AppError::forbidden(format!(
        "missing permission: {permission_code}"
    )))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("device admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::normalize_blacklist_reason;

    #[test]
    fn blacklist_reason_is_required_and_trimmed() {
        assert_eq!(
            normalize_blacklist_reason(Some(" abuse ".to_owned())).expect("reason"),
            "abuse"
        );
        assert!(normalize_blacklist_reason(None).is_err());
        assert!(normalize_blacklist_reason(Some(" ".to_owned())).is_err());
    }

    #[test]
    fn blacklist_reason_rejects_long_values() {
        assert!(normalize_blacklist_reason(Some("a".repeat(501))).is_err());
    }
}
