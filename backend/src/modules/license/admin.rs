use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    Extension, Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::repository::ApplicationRepository,
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        customer::repository::CustomerRepository,
        license::{
            model::{
                normalize_reset_device_reason, validate_license_status_filter,
                validate_renew_expires_at, CreateLicenseInput, License, LicenseListMeta,
                LicenseListQuery, LicenseSummary, NewLicense, RenewLicenseInput,
                ResetLicenseDevicesInput,
            },
            repository::LicenseRepository,
        },
    },
    state::AppState,
};

pub const MAX_RESET_LICENSE_DEVICES_BODY_BYTES: usize = 4 * 1024;

#[derive(Debug, Serialize)]
pub struct LicenseListResponse {
    pub items: Vec<LicenseSummary>,
    pub meta: LicenseListMeta,
}

#[derive(Debug, Serialize)]
pub struct CreateLicenseResponse {
    pub license_key: String,
    pub license: LicenseSummary,
}

#[derive(Debug, Serialize)]
pub struct LicenseMutationResponse {
    pub license: LicenseSummary,
    pub revoked_sessions: u64,
}

pub async fn list_licenses(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<LicenseListQuery>,
) -> Result<Json<ApiResponse<LicenseListResponse>>, AppError> {
    ensure_admin_permission(&admin, "license:read")?;

    validate_license_status_filter(query.status.as_deref())?;
    let licenses = LicenseRepository::new(state.db.clone())
        .list(admin.tenant_id, &query)
        .await?;
    let items = licenses.into_iter().map(LicenseSummary::from).collect();

    Ok(Json(ApiResponse::ok(
        LicenseListResponse {
            items,
            meta: LicenseListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn create_license(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateLicenseInput>,
) -> Result<Json<ApiResponse<CreateLicenseResponse>>, AppError> {
    ensure_admin_permission(&admin, "license:create")?;
    ensure_application_exists(&state, admin.tenant_id, payload.app_id).await?;
    if let Some(customer_id) = payload.customer_id {
        ensure_customer_exists(&state, admin.tenant_id, customer_id).await?;
    }

    let license_key = generate_license_key();
    let license_key_hash = hash_token(&state.config.security.token_hash_pepper, &license_key)?;
    let new_license = NewLicense::from_input(admin.tenant_id, payload, license_key_hash)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let license = create_license_in_transaction(&mut transaction, new_license).await?;
    audit_license_create(&mut transaction, &admin, &request_id, &license).await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CreateLicenseResponse {
            license_key,
            license: license.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn revoke_license(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(license_id): Path<Uuid>,
) -> Result<Json<ApiResponse<LicenseMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "license:revoke")?;

    let repository = LicenseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, license_id)
        .await?
        .ok_or_else(AppError::license_not_found)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let license = set_license_status_in_transaction(
        &mut transaction,
        admin.tenant_id,
        license_id,
        "revoked",
        true,
    )
    .await?
    .ok_or_else(|| AppError::already_revoked("license already revoked"))?;
    let revoked_refresh_tokens =
        revoke_license_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id, license_id)
            .await?;
    let revoked_sessions =
        revoke_license_sessions_in_transaction(&mut transaction, admin.tenant_id, license_id)
            .await?;
    audit_license_status_change(
        &mut transaction,
        &admin,
        &request_id,
        "license.revoke",
        &before,
        &license,
        revoked_sessions,
        revoked_refresh_tokens,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        LicenseMutationResponse {
            license: license.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn suspend_license(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(license_id): Path<Uuid>,
) -> Result<Json<ApiResponse<LicenseMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "license:suspend")?;

    let repository = LicenseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, license_id)
        .await?
        .ok_or_else(AppError::license_not_found)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let license = set_license_status_in_transaction(
        &mut transaction,
        admin.tenant_id,
        license_id,
        "suspended",
        false,
    )
    .await?
    .ok_or_else(|| AppError::conflict("license already suspended"))?;
    let revoked_refresh_tokens =
        revoke_license_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id, license_id)
            .await?;
    let revoked_sessions =
        revoke_license_sessions_in_transaction(&mut transaction, admin.tenant_id, license_id)
            .await?;
    audit_license_status_change(
        &mut transaction,
        &admin,
        &request_id,
        "license.suspend",
        &before,
        &license,
        revoked_sessions,
        revoked_refresh_tokens,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        LicenseMutationResponse {
            license: license.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn renew_license(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(license_id): Path<Uuid>,
    Json(payload): Json<RenewLicenseInput>,
) -> Result<Json<ApiResponse<LicenseMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "license:renew")?;

    let repository = LicenseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, license_id)
        .await?
        .ok_or_else(AppError::license_not_found)?;
    validate_renew_expires_at(&before, payload.expires_at)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let license = renew_license_in_transaction(
        &mut transaction,
        admin.tenant_id,
        license_id,
        payload.expires_at,
    )
    .await?
    .ok_or_else(|| AppError::already_revoked("revoked license cannot be renewed"))?;
    audit_license_status_change(
        &mut transaction,
        &admin,
        &request_id,
        "license.renew",
        &before,
        &license,
        0,
        0,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        LicenseMutationResponse {
            license: license.into(),
            revoked_sessions: 0,
        },
        request_id.to_string(),
    )))
}

pub async fn reset_license_devices(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(license_id): Path<Uuid>,
    body: Bytes,
) -> Result<Json<ApiResponse<LicenseMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "license:reset_device")?;
    let reason = parse_reset_license_devices_reason(&body)?;

    let license = LicenseRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, license_id)
        .await?
        .ok_or_else(AppError::license_not_found)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let devices_reset =
        reset_license_devices_in_transaction(&mut transaction, admin.tenant_id, license_id).await?;
    let revoked_refresh_tokens =
        revoke_license_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id, license_id)
            .await?;
    let revoked_sessions =
        revoke_license_sessions_in_transaction(&mut transaction, admin.tenant_id, license_id)
            .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "license.devices.reset",
            resource_type: "license",
            resource_id: Some(license.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: None,
            metadata_json: json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
                "devices_reset": devices_reset,
                "reason": reason,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        LicenseMutationResponse {
            license: license.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

async fn create_license_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    license: NewLicense,
) -> Result<License, AppError> {
    sqlx::query_as::<_, License>(
        r#"
        insert into licenses (
          id,
          tenant_id,
          app_id,
          customer_id,
          license_key_hash,
          type,
          max_devices,
          features,
          starts_at,
          expires_at,
          metadata
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          license_key_hash,
          type as license_type,
          status,
          max_devices,
          features,
          starts_at,
          expires_at,
          revoked_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(license.id)
    .bind(license.tenant_id)
    .bind(license.app_id)
    .bind(license.customer_id)
    .bind(license.license_key_hash)
    .bind(license.license_type)
    .bind(license.max_devices)
    .bind(license.features)
    .bind(license.starts_at)
    .bind(license.expires_at)
    .bind(license.metadata)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn reset_license_devices_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    license_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update devices
        set
          status = 'unbound',
          updated_at = now()
        where tenant_id = $1
          and license_id = $2
          and status <> 'unbound'
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(license_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn set_license_status_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    license_id: Uuid,
    status: &'static str,
    revoked: bool,
) -> Result<Option<License>, AppError> {
    sqlx::query_as::<_, License>(
        r#"
        update licenses
        set
          status = $3,
          revoked_at = case when $4 then now() else revoked_at end,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
          and status <> $3
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          license_key_hash,
          type as license_type,
          status,
          max_devices,
          features,
          starts_at,
          expires_at,
          revoked_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(license_id)
    .bind(status)
    .bind(revoked)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn renew_license_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    license_id: Uuid,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<Option<License>, AppError> {
    sqlx::query_as::<_, License>(
        r#"
        update licenses
        set
          status = 'active',
          expires_at = $3,
          revoked_at = null,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
          and status <> 'revoked'
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          license_key_hash,
          type as license_type,
          status,
          max_devices,
          features,
          starts_at,
          expires_at,
          revoked_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(license_id)
    .bind(expires_at)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn revoke_license_sessions_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    license_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_sessions
        set revoked_at = now()
        where tenant_id = $1
          and revoked_at is null
          and device_id in (
            select id
            from devices
            where tenant_id = $1
              and license_id = $2
              and deleted_at is null
          )
        "#,
    )
    .bind(tenant_id)
    .bind(license_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_license_refresh_tokens_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    license_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_refresh_tokens rt
        set revoked_at = now()
        from client_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and rt.revoked_at is null
          and s.device_id in (
            select id
            from devices
            where tenant_id = $1
              and license_id = $2
              and deleted_at is null
          )
        "#,
    )
    .bind(tenant_id)
    .bind(license_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

fn generate_license_key() -> String {
    format!("lic_{}", generate_token())
}

fn parse_reset_license_devices_reason(body: &[u8]) -> Result<String, AppError> {
    if body.is_empty() {
        return Err(AppError::validation_failed("reason is required"));
    }
    if body.len() > MAX_RESET_LICENSE_DEVICES_BODY_BYTES {
        return Err(AppError::validation_failed(
            "reset device payload is too large",
        ));
    }

    let payload = serde_json::from_slice::<ResetLicenseDevicesInput>(body)
        .map_err(|_| AppError::validation_failed("reset device payload is invalid"))?;

    normalize_reset_device_reason(payload.reason)
}

async fn ensure_application_exists(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
) -> Result<(), AppError> {
    ApplicationRepository::new(state.db.clone())
        .find_by_id(tenant_id, app_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;

    Ok(())
}

async fn ensure_customer_exists(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    CustomerRepository::new(state.db.clone())
        .find_by_id(tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;

    Ok(())
}

async fn audit_license_create(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    license: &License,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "license.create",
            resource_type: "license",
            resource_id: Some(license.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(license_audit_json(license)),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn audit_license_status_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: &License,
    license: &License,
    revoked_sessions: u64,
    revoked_refresh_tokens: u64,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "license",
            resource_id: Some(license.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(license_audit_json(before)),
            after_json: Some(license_audit_json(license)),
            metadata_json: json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await
}

fn license_audit_json(license: &License) -> Value {
    json!({
        "id": license.id,
        "app_id": license.app_id,
        "customer_id": license.customer_id,
        "type": license.license_type,
        "status": license.status,
        "max_devices": license.max_devices,
        "features": license.features,
        "starts_at": license.starts_at,
        "expires_at": license.expires_at,
        "revoked_at": license.revoked_at,
        "metadata": license.metadata,
    })
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
    AppError::dependency(format!("license admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        generate_license_key, parse_reset_license_devices_reason,
        MAX_RESET_LICENSE_DEVICES_BODY_BYTES,
    };

    #[test]
    fn generated_license_key_is_prefixed_high_entropy_token() {
        let license_key = generate_license_key();

        assert!(license_key.starts_with("lic_"));
        assert!(license_key.len() >= 44);
    }

    #[test]
    fn reset_license_devices_payload_requires_reason() {
        assert_eq!(
            parse_reset_license_devices_reason(br#"{ "reason": "device replacement" }"#)
                .expect("reason should parse"),
            "device replacement"
        );
        assert!(parse_reset_license_devices_reason(b"").is_err());
        assert!(parse_reset_license_devices_reason(br#"{ "reason": " " }"#).is_err());
        assert!(parse_reset_license_devices_reason(b"not json").is_err());
    }

    #[test]
    fn reset_license_devices_payload_rejects_oversized_body() {
        let body = vec![b'a'; MAX_RESET_LICENSE_DEVICES_BODY_BYTES + 1];

        assert!(parse_reset_license_devices_reason(&body).is_err());
    }
}
