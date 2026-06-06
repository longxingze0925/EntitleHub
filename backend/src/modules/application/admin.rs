use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    crypto::{
        envelope::encrypt_private_key,
        signing::generate_ed25519_key,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::{
            model::{
                validate_application_status_filter, Application, ApplicationListMeta,
                ApplicationListQuery, ApplicationSummary, CreateApplicationInput, NewApplication,
                NewSigningKey, SigningKey, UpdateApplication, UpdateApplicationInput,
            },
            repository::{
                create_application_in_transaction, create_signing_key_in_transaction,
                retire_active_app_request_keys_in_transaction,
                retire_active_global_signing_keys_in_transaction,
                update_application_in_transaction, update_application_secret_hash_in_transaction,
                ApplicationRepository,
            },
        },
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const JWT_ACCESS_TOKEN_KEY_SCOPE: &str = "jwt_access_token";

#[derive(Debug, Serialize)]
pub struct CreateApplicationResponse {
    pub id: Uuid,
    pub app_key: String,
    pub app_secret: String,
    pub signing_key: SigningKeyResponse,
    pub application: ApplicationSummary,
}

#[derive(Debug, Serialize)]
pub struct ApplicationListResponse {
    pub items: Vec<ApplicationSummary>,
    pub meta: ApplicationListMeta,
}

#[derive(Debug, Serialize)]
pub struct ApplicationDetailResponse {
    pub application: ApplicationSummary,
}

#[derive(Debug, Serialize)]
pub struct UpdateApplicationResponse {
    pub application: ApplicationSummary,
}

#[derive(Debug, Serialize)]
pub struct RotateApplicationKeysResponse {
    pub id: Uuid,
    pub app_key: String,
    pub app_secret: String,
    pub signing_key: SigningKeyResponse,
}

#[derive(Debug, Serialize)]
pub struct SigningKeyListResponse {
    pub items: Vec<SigningKeyResponse>,
}

#[derive(Debug, Serialize)]
pub struct RotateGlobalJwtSigningKeyResponse {
    pub signing_key: SigningKeyResponse,
    pub retired_key_count: usize,
}

#[derive(Debug, Serialize)]
pub struct SigningKeyResponse {
    pub id: Uuid,
    pub kid: String,
    pub key_scope: String,
    pub alg: String,
    pub public_key_pem: String,
    pub status: String,
    pub not_before: DateTime<Utc>,
    pub not_after: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
}

pub async fn list_applications(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<ApplicationListQuery>,
) -> Result<Json<ApiResponse<ApplicationListResponse>>, AppError> {
    ensure_admin_permission(&admin, "app:read")?;

    validate_application_status_filter(query.status.as_deref())?;
    let applications = ApplicationRepository::new(state.db.clone())
        .list(admin.tenant_id, &query)
        .await?;
    let items = applications
        .into_iter()
        .map(ApplicationSummary::from)
        .collect();

    Ok(Json(ApiResponse::ok(
        ApplicationListResponse {
            items,
            meta: ApplicationListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn get_application(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(application_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ApplicationDetailResponse>>, AppError> {
    ensure_admin_permission(&admin, "app:read")?;

    let application = ApplicationRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, application_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;

    Ok(Json(ApiResponse::ok(
        ApplicationDetailResponse {
            application: application.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn create_application(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateApplicationInput>,
) -> Result<Json<ApiResponse<CreateApplicationResponse>>, AppError> {
    ensure_admin_permission(&admin, "app:create")?;

    let app_key = generate_app_key();
    let app_secret = generate_token();
    let app_secret_hash = hash_token(&state.config.security.token_hash_pepper, &app_secret)?;
    let application_input =
        NewApplication::from_input(admin.tenant_id, payload, app_key.clone(), app_secret_hash)?;
    let generated_signing_key = generate_ed25519_key()?;
    let private_key_envelope = encrypt_private_key(
        &state.config.security.master_key,
        &generated_signing_key.private_key_pkcs8_der,
    )?;
    let private_key_envelope_json =
        serde_json::to_value(private_key_envelope).map_err(|error| {
            AppError::crypto(format!(
                "private key envelope serialization failed: {error}"
            ))
        })?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let application =
        create_application_in_transaction(&mut transaction, application_input).await?;
    let new_signing_key = NewSigningKey::app_request(
        admin.tenant_id,
        application.id,
        generated_signing_key.kid,
        generated_signing_key.public_key_pem,
        private_key_envelope_json,
        admin.team_member_id,
    );
    let signing_key = create_signing_key_in_transaction(&mut transaction, new_signing_key).await?;
    audit_application_create(
        &mut transaction,
        &admin,
        &request_id,
        &application,
        &signing_key,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CreateApplicationResponse {
            id: application.id,
            app_key,
            app_secret,
            signing_key: signing_key_response(signing_key),
            application: application.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn update_application(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(application_id): Path<Uuid>,
    Json(payload): Json<UpdateApplicationInput>,
) -> Result<Json<ApiResponse<UpdateApplicationResponse>>, AppError> {
    ensure_admin_permission(&admin, "app:update")?;

    let repository = ApplicationRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, application_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;
    let input = UpdateApplication::from_input(payload, &before)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let application =
        update_application_in_transaction(&mut transaction, admin.tenant_id, application_id, input)
            .await?
            .ok_or_else(AppError::app_not_found)?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "application.update",
            resource_type: "application",
            resource_id: Some(application.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(application_only_audit_json(&before)),
            after_json: Some(application_only_audit_json(&application)),
            metadata_json: json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        UpdateApplicationResponse {
            application: application.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn rotate_application_keys(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(application_id): Path<Uuid>,
) -> Result<Json<ApiResponse<RotateApplicationKeysResponse>>, AppError> {
    ensure_admin_permission(&admin, "app:rotate_key")?;

    let before = ApplicationRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, application_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;
    let app_secret = generate_token();
    let app_secret_hash = hash_token(&state.config.security.token_hash_pepper, &app_secret)?;
    let generated_signing_key = generate_ed25519_key()?;
    let private_key_envelope_json = encrypted_private_key_json(
        &state.config.security.master_key,
        &generated_signing_key.private_key_pkcs8_der,
    )?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let retired_keys =
        retire_active_app_request_keys_in_transaction(&mut transaction, admin.tenant_id, before.id)
            .await?;
    let rotated_from_id = retired_keys.first().map(|key| key.id);
    let application = update_application_secret_hash_in_transaction(
        &mut transaction,
        admin.tenant_id,
        before.id,
        &app_secret_hash,
    )
    .await?;
    let new_signing_key = NewSigningKey::app_request(
        admin.tenant_id,
        application.id,
        generated_signing_key.kid,
        generated_signing_key.public_key_pem,
        private_key_envelope_json,
        admin.team_member_id,
    )
    .with_rotated_from_id(rotated_from_id);
    let signing_key = create_signing_key_in_transaction(&mut transaction, new_signing_key).await?;
    audit_application_rotate_keys(
        &mut transaction,
        &admin,
        &request_id,
        &before,
        &application,
        &retired_keys,
        &signing_key,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RotateApplicationKeysResponse {
            id: application.id,
            app_key: application.app_key,
            app_secret,
            signing_key: signing_key_response(signing_key),
        },
        request_id.to_string(),
    )))
}

pub async fn list_application_signing_keys(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(application_id): Path<Uuid>,
) -> Result<Json<ApiResponse<SigningKeyListResponse>>, AppError> {
    ensure_admin_permission(&admin, "app:read_key")?;

    let repository = ApplicationRepository::new(state.db.clone());
    repository
        .find_by_id(admin.tenant_id, application_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;
    let items = repository
        .list_signing_keys(admin.tenant_id, application_id)
        .await?
        .into_iter()
        .map(signing_key_response)
        .collect();

    Ok(Json(ApiResponse::ok(
        SigningKeyListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn list_global_jwt_signing_keys(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<SigningKeyListResponse>>, AppError> {
    ensure_admin_permission(&admin, "security:read")?;

    let items = ApplicationRepository::new(state.db.clone())
        .list_global_signing_keys(JWT_ACCESS_TOKEN_KEY_SCOPE)
        .await?
        .into_iter()
        .map(signing_key_response)
        .collect();

    Ok(Json(ApiResponse::ok(
        SigningKeyListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn rotate_global_jwt_signing_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<RotateGlobalJwtSigningKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "security:rotate_key")?;

    let generated_signing_key = generate_ed25519_key()?;
    let private_key_envelope_json = encrypted_private_key_json(
        &state.config.security.master_key,
        &generated_signing_key.private_key_pkcs8_der,
    )?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let retiring_not_after =
        Utc::now() + Duration::seconds(state.config.security.client_access_token_ttl_seconds);
    let retired_keys = retire_active_global_signing_keys_in_transaction(
        &mut transaction,
        JWT_ACCESS_TOKEN_KEY_SCOPE,
        retiring_not_after,
    )
    .await?;
    let retired_key_count = retired_keys.len();
    let rotated_from_id = retired_keys.first().map(|key| key.id);
    let new_signing_key = NewSigningKey::jwt_access_token(
        generated_signing_key.kid,
        generated_signing_key.public_key_pem,
        private_key_envelope_json,
    )
    .with_rotated_from_id(rotated_from_id)
    .with_created_by(Some(admin.team_member_id));
    let signing_key = create_signing_key_in_transaction(&mut transaction, new_signing_key).await?;
    audit_global_jwt_signing_key_rotate(
        &mut transaction,
        &admin,
        &request_id,
        &retired_keys,
        &signing_key,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RotateGlobalJwtSigningKeyResponse {
            signing_key: signing_key_response(signing_key),
            retired_key_count,
        },
        request_id.to_string(),
    )))
}

fn generate_app_key() -> String {
    format!("app_{}", generate_token())
}

fn encrypted_private_key_json(
    master_key: &[u8; 32],
    private_key: &[u8],
) -> Result<Value, AppError> {
    let private_key_envelope = encrypt_private_key(master_key, private_key)?;

    serde_json::to_value(private_key_envelope).map_err(|error| {
        AppError::crypto(format!(
            "private key envelope serialization failed: {error}"
        ))
    })
}

fn signing_key_response(signing_key: SigningKey) -> SigningKeyResponse {
    SigningKeyResponse {
        id: signing_key.id,
        kid: signing_key.kid,
        key_scope: signing_key.key_scope,
        alg: signing_key.alg,
        public_key_pem: signing_key.public_key_pem,
        status: signing_key.status,
        not_before: signing_key.not_before,
        not_after: signing_key.not_after,
        created_at: signing_key.created_at,
        activated_at: signing_key.activated_at,
    }
}

async fn audit_application_create(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    application: &Application,
    signing_key: &SigningKey,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "application.create",
            resource_type: "application",
            resource_id: Some(application.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(application_audit_json(application, signing_key)),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn audit_application_rotate_keys(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    before: &Application,
    application: &Application,
    retired_keys: &[SigningKey],
    signing_key: &SigningKey,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "application.keys.rotate",
            resource_type: "application",
            resource_id: Some(application.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "id": before.id,
                "app_key": before.app_key,
                "retired_key_ids": retired_keys.iter().map(|key| key.id).collect::<Vec<_>>(),
            })),
            after_json: Some(json!({
                "id": application.id,
                "app_key": application.app_key,
                "new_signing_key": signing_key_audit_json(signing_key),
            })),
            metadata_json: json!({
                "retired_key_count": retired_keys.len(),
            }),
        },
    )
    .await
}

async fn audit_global_jwt_signing_key_rotate(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    retired_keys: &[SigningKey],
    signing_key: &SigningKey,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "jwt.signing_key.rotate",
            resource_type: "signing_key",
            resource_id: Some(signing_key.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "retired_key_ids": retired_keys.iter().map(|key| key.id).collect::<Vec<_>>(),
                "retired_kids": retired_keys.iter().map(|key| key.kid.clone()).collect::<Vec<_>>(),
            })),
            after_json: Some(signing_key_audit_json(signing_key)),
            metadata_json: json!({
                "key_scope": JWT_ACCESS_TOKEN_KEY_SCOPE,
                "retired_key_count": retired_keys.len(),
            }),
        },
    )
    .await
}

fn application_audit_json(application: &Application, signing_key: &SigningKey) -> Value {
    json!({
        "id": application.id,
        "name": application.name,
        "slug": application.slug,
        "app_key": application.app_key,
        "auth_mode": application.auth_mode,
        "status": application.status,
        "heartbeat_interval_seconds": application.heartbeat_interval_seconds,
        "offline_tolerance_seconds": application.offline_tolerance_seconds,
        "max_devices_default": application.max_devices_default,
        "metadata": application.metadata,
        "signing_key": {
            "id": signing_key.id,
            "kid": signing_key.kid,
            "key_scope": signing_key.key_scope,
            "alg": signing_key.alg,
            "status": signing_key.status,
        },
    })
}

fn application_only_audit_json(application: &Application) -> Value {
    json!({
        "id": application.id,
        "name": application.name,
        "slug": application.slug,
        "app_key": application.app_key,
        "auth_mode": application.auth_mode,
        "status": application.status,
        "heartbeat_interval_seconds": application.heartbeat_interval_seconds,
        "offline_tolerance_seconds": application.offline_tolerance_seconds,
        "max_devices_default": application.max_devices_default,
        "metadata": application.metadata,
    })
}

fn signing_key_audit_json(signing_key: &SigningKey) -> Value {
    json!({
        "id": signing_key.id,
        "kid": signing_key.kid,
        "key_scope": signing_key.key_scope,
        "alg": signing_key.alg,
        "status": signing_key.status,
        "rotated_from_id": signing_key.rotated_from_id,
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
    AppError::dependency(format!("application admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::generate_app_key;

    #[test]
    fn generated_app_key_is_public_prefixed_token() {
        let app_key = generate_app_key();

        assert!(app_key.starts_with("app_"));
        assert!(app_key.len() >= 44);
    }
}
