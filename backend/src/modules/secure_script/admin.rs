use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    crypto::{
        envelope::{decrypt_private_key, encrypt_bytes, encrypt_private_key, PrivateKeyEnvelope},
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
        secure_script::{
            model::{
                empty_script_sha256, secure_script_signature_payload,
                validate_create_secure_script_input, validate_secure_script_status_filter,
                validate_update_secure_script_content_input, CreateSecureScriptInput,
                NewSecureScript, SecureScript, SecureScriptListMeta, SecureScriptListQuery,
                SecureScriptSummary, UpdateSecureScriptContent, UpdateSecureScriptContentInput,
            },
            repository::{
                create_secure_script_in_transaction, deprecate_secure_script_in_transaction,
                publish_secure_script_in_transaction, update_secure_script_content_in_transaction,
                SecureScriptRepository,
            },
        },
    },
    state::AppState,
};

const MAX_SCRIPT_CONTENT_BYTES: usize = 1024 * 1024;
const MAX_SCRIPT_CONTENT_BASE64_CHARS: usize = (((MAX_SCRIPT_CONTENT_BYTES + 2) / 3) * 4) + 4;

#[derive(Debug, Serialize)]
pub struct SecureScriptResponse {
    pub script: SecureScriptSummary,
}

#[derive(Debug, Serialize)]
pub struct SecureScriptListResponse {
    pub items: Vec<SecureScriptSummary>,
    pub meta: SecureScriptListMeta,
}

pub async fn list_secure_scripts(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(app_id): Path<Uuid>,
    Query(query): Query<SecureScriptListQuery>,
) -> Result<Json<ApiResponse<SecureScriptListResponse>>, AppError> {
    ensure_admin_permission(&admin, "script:read")?;
    validate_secure_script_status_filter(query.status.as_deref())?;
    ensure_application_exists(&state, admin.tenant_id, app_id).await?;

    let scripts = SecureScriptRepository::new(state.db.clone())
        .list(admin.tenant_id, app_id, &query)
        .await?;
    let items = scripts.into_iter().map(SecureScriptSummary::from).collect();

    Ok(Json(ApiResponse::ok(
        SecureScriptListResponse {
            items,
            meta: SecureScriptListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn create_secure_script(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(app_id): Path<Uuid>,
    Json(payload): Json<CreateSecureScriptInput>,
) -> Result<Json<ApiResponse<SecureScriptResponse>>, AppError> {
    ensure_admin_permission(&admin, "script:create")?;
    ensure_application_exists(&state, admin.tenant_id, app_id).await?;
    validate_create_secure_script_input(&payload)?;

    let signing_key =
        get_or_create_secure_script_signing_key(&state, &admin, &request_id, app_id).await?;
    let signature = sign_payload_with_key(
        &state,
        &signing_key,
        &secure_script_signature_payload(empty_script_sha256(), payload.version_code),
    )?;
    let encrypted_empty_content = encrypt_content_to_text(&state, b"")?;
    let new_script = NewSecureScript::from_input(
        admin.tenant_id,
        app_id,
        payload,
        encrypted_empty_content,
        signing_key.id,
        signing_key.kid.clone(),
        signature,
    )?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let script = create_secure_script_in_transaction(&mut transaction, new_script).await?;
    audit_script_change(
        &mut transaction,
        &admin,
        &request_id,
        "secure_script.create",
        None,
        &script,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        SecureScriptResponse {
            script: script.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn update_secure_script_content(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(script_id): Path<Uuid>,
    Json(payload): Json<UpdateSecureScriptContentInput>,
) -> Result<Json<ApiResponse<SecureScriptResponse>>, AppError> {
    ensure_admin_permission(&admin, "script:update")?;

    let repository = SecureScriptRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, script_id)
        .await?
        .ok_or_else(AppError::script_not_found)?;
    validate_update_secure_script_content_input(&payload)?;
    let content = decode_content_base64(&payload.content_base64)?;
    let content_sha256 = sha256_hex(&content);
    let version_code = payload.version_code.unwrap_or(before.version_code);
    let signing_key =
        get_or_create_secure_script_signing_key(&state, &admin, &request_id, before.app_id).await?;
    let signature = sign_payload_with_key(
        &state,
        &signing_key,
        &secure_script_signature_payload(&content_sha256, version_code),
    )?;
    let encrypted_content = encrypt_content_to_text(&state, &content)?;
    let input = UpdateSecureScriptContent::new(
        encrypted_content,
        content_sha256,
        signing_key.id,
        signing_key.kid.clone(),
        signature,
        payload,
    )?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let script = update_secure_script_content_in_transaction(
        &mut transaction,
        admin.tenant_id,
        script_id,
        input,
    )
    .await?
    .ok_or_else(AppError::script_not_found)?;
    audit_script_change(
        &mut transaction,
        &admin,
        &request_id,
        "secure_script.content.update",
        Some(&before),
        &script,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        SecureScriptResponse {
            script: script.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn publish_secure_script(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(script_id): Path<Uuid>,
) -> Result<Json<ApiResponse<SecureScriptResponse>>, AppError> {
    mutate_script_status(
        state,
        admin,
        request_id,
        script_id,
        "script:publish",
        "secure_script.publish",
        true,
    )
    .await
}

pub async fn deprecate_secure_script(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(script_id): Path<Uuid>,
) -> Result<Json<ApiResponse<SecureScriptResponse>>, AppError> {
    mutate_script_status(
        state,
        admin,
        request_id,
        script_id,
        "script:deprecate",
        "secure_script.deprecate",
        false,
    )
    .await
}

async fn mutate_script_status(
    state: AppState,
    admin: AdminContext,
    request_id: RequestId,
    script_id: Uuid,
    permission: &'static str,
    action: &'static str,
    publish: bool,
) -> Result<Json<ApiResponse<SecureScriptResponse>>, AppError> {
    ensure_admin_permission(&admin, permission)?;

    let repository = SecureScriptRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, script_id)
        .await?
        .ok_or_else(AppError::script_not_found)?;
    let (expected_status, invalid_state_message) = if publish {
        ("draft", "only draft script can be published")
    } else {
        ("published", "only published script can be deprecated")
    };
    ensure_script_state(&before, expected_status, invalid_state_message)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let script = if publish {
        publish_secure_script_in_transaction(&mut transaction, admin.tenant_id, script_id).await?
    } else {
        deprecate_secure_script_in_transaction(&mut transaction, admin.tenant_id, script_id).await?
    }
    .ok_or_else(|| AppError::invalid_script_state(invalid_state_message))?;

    audit_script_change(
        &mut transaction,
        &admin,
        &request_id,
        action,
        Some(&before),
        &script,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        SecureScriptResponse {
            script: script.into(),
        },
        request_id.to_string(),
    )))
}

fn decode_content_base64(content_base64: &str) -> Result<Vec<u8>, AppError> {
    let content_base64 = content_base64.trim();
    if content_base64.len() > MAX_SCRIPT_CONTENT_BASE64_CHARS {
        return Err(AppError::validation_failed("content_base64 is too large"));
    }

    let content = STANDARD
        .decode(content_base64)
        .map_err(|_| AppError::validation_failed("content_base64 is invalid"))?;
    if content.len() > MAX_SCRIPT_CONTENT_BYTES {
        return Err(AppError::validation_failed("content_base64 is too large"));
    }

    Ok(content)
}

fn sha256_hex(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

fn encrypt_content_to_text(state: &AppState, content: &[u8]) -> Result<String, AppError> {
    let envelope = encrypt_bytes(&state.config.security.master_key, content)?;

    serde_json::to_string(&envelope)
        .map_err(|error| AppError::crypto(format!("script envelope serialization failed: {error}")))
}

fn sign_payload_with_key(
    state: &AppState,
    signing_key: &SigningKey,
    payload: &str,
) -> Result<String, AppError> {
    let private_key_envelope = signing_key
        .private_key_envelope
        .clone()
        .ok_or_else(|| AppError::crypto("script signing key private envelope missing"))?;
    let private_key_envelope: PrivateKeyEnvelope = serde_json::from_value(private_key_envelope)
        .map_err(|error| {
            AppError::crypto(format!("script signing key envelope invalid: {error}"))
        })?;
    let private_key =
        decrypt_private_key(&state.config.security.master_key, &private_key_envelope)?;

    sign_ed25519(&private_key, payload.as_bytes())
}

async fn get_or_create_secure_script_signing_key(
    state: &AppState,
    admin: &AdminContext,
    request_id: &RequestId,
    app_id: Uuid,
) -> Result<SigningKey, AppError> {
    let repository = ApplicationRepository::new(state.db.clone());
    if let Some(signing_key) = repository
        .find_active_signing_key_with_private_envelope(admin.tenant_id, app_id, "secure_script")
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
    let new_signing_key = NewSigningKey::secure_script(
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
            action: "secure_script.signing_key.create",
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

async fn audit_script_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: Option<&SecureScript>,
    script: &SecureScript,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "secure_script",
            resource_id: Some(script.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: before.map(script_audit_json),
            after_json: Some(script_audit_json(script)),
            metadata_json: json!({}),
        },
    )
    .await
}

fn script_audit_json(script: &SecureScript) -> Value {
    json!({
        "id": script.id,
        "app_id": script.app_id,
        "name": script.name,
        "version": script.version,
        "version_code": script.version_code,
        "status": script.status,
        "content_sha256": script.content_sha256,
        "signature_kid": script.signature_kid,
        "signature_alg": script.signature_alg,
        "required_features": script.required_features,
        "expires_at": script.expires_at,
        "published_at": script.published_at,
    })
}

fn ensure_script_state(
    script: &SecureScript,
    expected_status: &'static str,
    message: &'static str,
) -> Result<(), AppError> {
    if script.status == expected_status {
        return Ok(());
    }

    Err(AppError::invalid_script_state(message))
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
    AppError::dependency(format!("secure script admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use chrono::Utc;
    use uuid::Uuid;

    use super::{
        decode_content_base64, ensure_script_state, sha256_hex, MAX_SCRIPT_CONTENT_BASE64_CHARS,
        MAX_SCRIPT_CONTENT_BYTES,
    };
    use crate::{error::AppError, modules::secure_script::model::SecureScript};

    #[test]
    fn content_base64_decodes_script_bytes() {
        let content = decode_content_base64("YWJj").expect("decode");

        assert_eq!(content, b"abc");
        assert!(decode_content_base64("@@@").is_err());
    }

    #[test]
    fn content_base64_rejects_oversized_input() {
        let oversized_base64 = "A".repeat(MAX_SCRIPT_CONTENT_BASE64_CHARS + 1);
        assert!(decode_content_base64(&oversized_base64).is_err());

        let oversized_content = vec![0_u8; MAX_SCRIPT_CONTENT_BYTES + 1];
        let encoded = STANDARD.encode(oversized_content);
        assert!(decode_content_base64(&encoded).is_err());
    }

    #[test]
    fn sha256_hex_uses_lowercase_hex() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn script_state_guard_rejects_unexpected_status() {
        let script = test_script("published");

        assert!(matches!(
            ensure_script_state(&script, "draft", "only draft script can be published"),
            Err(AppError::InvalidScriptState(_))
        ));
    }

    fn test_script(status: &str) -> SecureScript {
        SecureScript {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            name: "script".to_owned(),
            version: "1.0.0".to_owned(),
            version_code: 1,
            status: status.to_owned(),
            content_ciphertext: "ciphertext".to_owned(),
            content_sha256: "sha256".to_owned(),
            signing_key_id: Uuid::nil(),
            signature_kid: "kid".to_owned(),
            signature: "signature".to_owned(),
            signature_alg: "Ed25519".to_owned(),
            required_features: serde_json::json!([]),
            expires_at: None,
            published_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }
}
