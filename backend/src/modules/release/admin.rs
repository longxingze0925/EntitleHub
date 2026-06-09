use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    crypto::{
        envelope::{decrypt_private_key, encrypt_private_key, PrivateKeyEnvelope},
        signing::{generate_ed25519_key, sign_ed25519},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::{
            model::{NewSigningKey, SigningKey},
            repository::{create_signing_key_in_transaction, ApplicationRepository},
        },
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        release::{
            model::{
                release_file_signature_payload, release_metadata_signature_payload,
                validate_register_release_file_input, validate_release_status_filter,
                CreateReleaseInput, NewRelease, NewReleaseFile, RegisterReleaseFileInput, Release,
                ReleaseFileSummary, ReleaseListMeta, ReleaseListQuery, ReleaseSummary,
                UpdateRelease, UpdateReleaseInput,
            },
            repository::{
                create_release_file_in_transaction, create_release_in_transaction,
                delete_draft_release_in_transaction, deprecate_release_in_transaction,
                publish_release_in_transaction, sign_release_in_transaction,
                update_release_in_transaction, ReleaseRepository,
            },
        },
    },
    state::AppState,
};

pub const MAX_RELEASE_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct RegisterReleaseFileResponse {
    pub file_id: Uuid,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub file: ReleaseFileSummary,
}

#[derive(Debug, Serialize)]
pub struct ReleaseListResponse {
    pub items: Vec<ReleaseSummary>,
    pub meta: ReleaseListMeta,
}

#[derive(Debug, Serialize)]
pub struct ReleaseResponse {
    pub release: ReleaseSummary,
}

#[derive(Debug, Serialize)]
pub struct ReleaseDetailResponse {
    pub release: ReleaseSummary,
    pub file: ReleaseFileSummary,
}

#[derive(Debug, Serialize)]
pub struct ReleaseDeleteResponse {
    pub deleted: bool,
    pub release_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct UploadReleaseFileQuery {
    pub file_name: String,
}

pub async fn register_release_file(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<RegisterReleaseFileInput>,
) -> Result<Json<ApiResponse<RegisterReleaseFileResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:upload")?;
    ensure_application_exists(&state, admin.tenant_id, app_id).await?;

    let response = create_signed_release_file(&state, &admin, &request_id, app_id, payload).await?;

    Ok(Json(ApiResponse::ok(response, request_id.to_string())))
}

pub async fn upload_release_file(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<UploadReleaseFileQuery>,
    body: Bytes,
) -> Result<Json<ApiResponse<RegisterReleaseFileResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:upload")?;
    ensure_application_exists(&state, admin.tenant_id, app_id).await?;
    if body.is_empty() {
        return Err(AppError::validation_failed("release file is required"));
    }
    if body.len() > MAX_RELEASE_UPLOAD_BYTES {
        return Err(AppError::validation_failed("release file is too large"));
    }

    let upload_id = Uuid::new_v4();
    let storage_key = format!(
        "tenants/{}/apps/{}/releases/uploads/{}",
        admin.tenant_id, app_id, upload_id
    );
    let sha256 = format!("{:x}", Sha256::digest(&body));
    let payload = RegisterReleaseFileInput {
        storage_key: Some(storage_key.clone()),
        file_name: query.file_name,
        file_size: body.len() as i64,
        sha256,
        metadata: None,
    };
    validate_register_release_file_input(&payload, admin.tenant_id, app_id)?;
    state.object_store.put_bytes(&storage_key, &body).await?;
    let response = create_signed_release_file(&state, &admin, &request_id, app_id, payload).await?;

    Ok(Json(ApiResponse::ok(response, request_id.to_string())))
}

async fn create_signed_release_file(
    state: &AppState,
    admin: &AdminContext,
    request_id: &RequestId,
    app_id: Uuid,
    payload: RegisterReleaseFileInput,
) -> Result<RegisterReleaseFileResponse, AppError> {
    validate_register_release_file_input(&payload, admin.tenant_id, app_id)?;

    let signing_key =
        get_or_create_release_file_signing_key(state, admin, request_id, app_id).await?;
    let normalized_payload = normalize_file_for_signature(&payload)?;
    let signature = sign_payload_with_key(state, &signing_key, &normalized_payload)?;
    let new_file = NewReleaseFile::from_input(
        admin.tenant_id,
        app_id,
        payload,
        signing_key.id,
        signing_key.kid.clone(),
        signature,
    )?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let file = create_release_file_in_transaction(&mut transaction, new_file).await?;
    let summary = ReleaseFileSummary::from(file.clone());

    audit_release_file_register(&mut transaction, admin, request_id, &summary).await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(RegisterReleaseFileResponse {
        file_id: summary.id,
        file_name: summary.file_name.clone(),
        file_size: summary.file_size,
        sha256: summary.sha256.clone(),
        signature_kid: summary.signature_kid.clone(),
        signature: summary.signature.clone(),
        signature_alg: summary.signature_alg.clone(),
        file: summary,
    })
}

pub async fn list_releases(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<ReleaseListQuery>,
) -> Result<Json<ApiResponse<ReleaseListResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:read")?;
    validate_release_status_filter(query.status.as_deref())?;
    ensure_application_exists(&state, admin.tenant_id, app_id).await?;

    let releases = ReleaseRepository::new(state.db.clone())
        .list(admin.tenant_id, app_id, &query)
        .await?;
    let items = releases.into_iter().map(ReleaseSummary::from).collect();

    Ok(Json(ApiResponse::ok(
        ReleaseListResponse {
            items,
            meta: ReleaseListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn get_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(release_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ReleaseDetailResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:read")?;

    let repository = ReleaseRepository::new(state.db.clone());
    let release = repository
        .find_by_id(admin.tenant_id, release_id)
        .await?
        .ok_or_else(AppError::release_not_found)?;
    let file = repository
        .find_file_by_id(admin.tenant_id, release.app_id, release.file_id)
        .await?
        .ok_or_else(|| AppError::not_found("release file not found"))?;

    Ok(Json(ApiResponse::ok(
        ReleaseDetailResponse {
            release: release.into(),
            file: file.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn create_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<CreateReleaseInput>,
) -> Result<Json<ApiResponse<ReleaseResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:create")?;
    ensure_application_exists(&state, admin.tenant_id, app_id).await?;

    let repository = ReleaseRepository::new(state.db.clone());
    repository
        .find_file_by_id(admin.tenant_id, app_id, payload.file_id)
        .await?
        .ok_or_else(|| AppError::not_found("release file not found"))?;
    let new_release = NewRelease::from_input(admin.tenant_id, app_id, payload)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let release = create_release_in_transaction(&mut transaction, new_release).await?;
    audit_release_create(&mut transaction, &admin, &request_id, &release).await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ReleaseResponse {
            release: release.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn update_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(release_id): Path<Uuid>,
    Json(payload): Json<UpdateReleaseInput>,
) -> Result<Json<ApiResponse<ReleaseResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:update")?;

    let repository = ReleaseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, release_id)
        .await?
        .ok_or_else(AppError::release_not_found)?;
    ensure_release_state(&before, "draft", "only draft release can be updated")?;
    let update = UpdateRelease::from_input(payload)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let release =
        update_release_in_transaction(&mut transaction, admin.tenant_id, release_id, update)
            .await?
            .ok_or_else(|| AppError::invalid_release_state("only draft release can be updated"))?;
    audit_release_change(
        &mut transaction,
        &admin,
        &request_id,
        "release.update",
        Some(&before),
        &release,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ReleaseResponse {
            release: release.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn publish_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(release_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ReleaseResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:publish")?;

    let repository = ReleaseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, release_id)
        .await?
        .ok_or_else(AppError::release_not_found)?;
    ensure_release_state(&before, "draft", "only draft release can be published")?;
    let file = repository
        .find_file_by_id(admin.tenant_id, before.app_id, before.file_id)
        .await?
        .ok_or_else(|| AppError::not_found("release file not found"))?;
    let signing_key =
        get_or_create_release_file_signing_key(&state, &admin, &request_id, before.app_id).await?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let published = publish_release_in_transaction(&mut transaction, admin.tenant_id, release_id)
        .await?
        .ok_or_else(|| AppError::invalid_release_state("only draft release can be published"))?;
    let published_at = published
        .published_at
        .ok_or_else(|| AppError::dependency("published release missing published_at"))?;
    let metadata_payload = release_metadata_signature_payload(
        published.app_id,
        &published.version,
        published.version_code,
        &file.sha256,
        file.file_size,
        published_at.timestamp(),
    );
    let signature = sign_payload_with_key(&state, &signing_key, &metadata_payload)?;
    let release = sign_release_in_transaction(
        &mut transaction,
        admin.tenant_id,
        release_id,
        signing_key.id,
        signing_key.kid.clone(),
        signature,
    )
    .await?
    .ok_or_else(|| AppError::dependency("published release signature write failed"))?;
    audit_release_status_change(
        &mut transaction,
        &admin,
        &request_id,
        "release.publish",
        &before,
        &release,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ReleaseResponse {
            release: release.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn deprecate_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(release_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ReleaseResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:deprecate")?;

    let repository = ReleaseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, release_id)
        .await?
        .ok_or_else(AppError::release_not_found)?;
    ensure_release_state(
        &before,
        "published",
        "only published release can be deprecated",
    )?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let release = deprecate_release_in_transaction(&mut transaction, admin.tenant_id, release_id)
        .await?
        .ok_or_else(|| {
            AppError::invalid_release_state("only published release can be deprecated")
        })?;
    audit_release_status_change(
        &mut transaction,
        &admin,
        &request_id,
        "release.deprecate",
        &before,
        &release,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ReleaseResponse {
            release: release.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn delete_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(release_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ReleaseDeleteResponse>>, AppError> {
    ensure_admin_permission(&admin, "release:delete")?;

    let repository = ReleaseRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, release_id)
        .await?
        .ok_or_else(AppError::release_not_found)?;
    ensure_release_state(&before, "draft", "only draft release can be deleted")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let release =
        delete_draft_release_in_transaction(&mut transaction, admin.tenant_id, release_id)
            .await?
            .ok_or_else(|| AppError::invalid_release_state("only draft release can be deleted"))?;
    audit_release_change(
        &mut transaction,
        &admin,
        &request_id,
        "release.delete",
        Some(&before),
        &release,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ReleaseDeleteResponse {
            deleted: true,
            release_id,
        },
        request_id.to_string(),
    )))
}

fn normalize_file_for_signature(input: &RegisterReleaseFileInput) -> Result<String, AppError> {
    let sha256 = input.sha256.trim().to_lowercase();
    if sha256.len() != 64 || !sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(AppError::validation_failed(
            "sha256 must be a 64-character hex string",
        ));
    }
    if input.file_size <= 0 {
        return Err(AppError::validation_failed(
            "file_size must be greater than 0",
        ));
    }

    Ok(release_file_signature_payload(&sha256, input.file_size))
}

fn sign_payload_with_key(
    state: &AppState,
    signing_key: &SigningKey,
    payload: &str,
) -> Result<String, AppError> {
    let private_key_envelope = signing_key
        .private_key_envelope
        .clone()
        .ok_or_else(|| AppError::crypto("release signing key private envelope missing"))?;
    let private_key_envelope: PrivateKeyEnvelope = serde_json::from_value(private_key_envelope)
        .map_err(|error| {
            AppError::crypto(format!("release signing key envelope invalid: {error}"))
        })?;
    let private_key =
        decrypt_private_key(&state.config.security.master_key, &private_key_envelope)?;

    sign_ed25519(&private_key, payload.as_bytes())
}

async fn get_or_create_release_file_signing_key(
    state: &AppState,
    admin: &AdminContext,
    request_id: &RequestId,
    app_id: Uuid,
) -> Result<SigningKey, AppError> {
    let repository = ApplicationRepository::new(state.db.clone());
    if let Some(signing_key) = repository
        .find_active_signing_key_with_private_envelope(admin.tenant_id, app_id, "release_file")
        .await?
    {
        return Ok(signing_key);
    }

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
    let new_signing_key = NewSigningKey::release_file(
        admin.tenant_id,
        app_id,
        generated_signing_key.kid,
        generated_signing_key.public_key_pem,
        private_key_envelope_json,
        admin.team_member_id,
    );

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let signing_key = create_signing_key_in_transaction(&mut transaction, new_signing_key).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "release.signing_key.create",
            resource_type: "signing_key",
            resource_id: Some(signing_key.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(json!({
                "id": signing_key.id,
                "app_id": app_id,
                "kid": signing_key.kid,
                "key_scope": signing_key.key_scope,
                "status": signing_key.status,
            })),
            metadata_json: json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(signing_key)
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

async fn audit_release_file_register(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    file: &ReleaseFileSummary,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "release_file.register",
            resource_type: "release_file",
            resource_id: Some(file.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(json!({
                "id": file.id,
                "file_name": file.file_name,
                "file_size": file.file_size,
                "sha256": file.sha256,
                "signature_kid": file.signature_kid,
                "signature_alg": file.signature_alg,
            })),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn audit_release_create(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    release: &Release,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "release.create",
            resource_type: "release",
            resource_id: Some(release.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(release_audit_json(release)),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn audit_release_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: Option<&Release>,
    release: &Release,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "release",
            resource_id: Some(release.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: before.map(release_audit_json),
            after_json: Some(release_audit_json(release)),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn audit_release_status_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: &Release,
    release: &Release,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "release",
            resource_id: Some(release.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(release_audit_json(before)),
            after_json: Some(release_audit_json(release)),
            metadata_json: json!({}),
        },
    )
    .await
}

fn release_audit_json(release: &Release) -> Value {
    json!({
        "id": release.id,
        "app_id": release.app_id,
        "file_id": release.file_id,
        "version": release.version,
        "version_code": release.version_code,
        "status": release.status,
        "force_update": release.force_update,
        "signature_kid": release.signature_kid,
        "signature_alg": release.signature_alg,
        "published_at": release.published_at,
        "deprecated_at": release.deprecated_at,
    })
}

fn ensure_release_state(
    release: &Release,
    expected_status: &'static str,
    message: &'static str,
) -> Result<(), AppError> {
    if release.status == expected_status {
        return Ok(());
    }

    Err(AppError::invalid_release_state(message))
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
    AppError::dependency(format!("release admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::{ensure_release_state, normalize_file_for_signature};
    use crate::{
        error::AppError,
        modules::release::model::{RegisterReleaseFileInput, Release},
    };

    #[test]
    fn release_file_signature_payload_uses_normalized_hash_and_size() {
        let payload = RegisterReleaseFileInput {
            storage_key: None,
            file_name: "app.zip".to_owned(),
            file_size: 123,
            sha256: "A".repeat(64),
            metadata: None,
        };

        assert_eq!(
            normalize_file_for_signature(&payload).expect("signature payload"),
            format!("{}:123", "a".repeat(64))
        );
    }

    #[test]
    fn release_state_guard_rejects_unexpected_status() {
        let release = test_release("published");

        assert!(matches!(
            ensure_release_state(&release, "draft", "only draft release can be published"),
            Err(AppError::InvalidReleaseState(_))
        ));
    }

    fn test_release(status: &str) -> Release {
        Release {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            file_id: Uuid::nil(),
            version: "1.0.0".to_owned(),
            version_code: 1,
            status: status.to_owned(),
            changelog: None,
            force_update: false,
            signing_key_id: None,
            signature_kid: None,
            signature: None,
            signature_alg: None,
            published_at: None,
            deprecated_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }
}
