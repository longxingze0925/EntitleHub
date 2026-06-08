use axum::{extract::State, Extension, Json};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, PrivateKeyEnvelope},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        client_auth::session::{ensure_active_entitlement, ClientContext},
        secure_script::{
            model::{
                ensure_required_features, FetchSecureScriptInput, SecureScript,
                SecureScriptVersionSummary,
            },
            repository::SecureScriptRepository,
        },
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct SecureScriptVersionsResponse {
    pub items: Vec<SecureScriptVersionSummary>,
}

#[derive(Debug, Serialize)]
pub struct FetchSecureScriptResponse {
    pub script_id: Uuid,
    pub version: String,
    pub version_code: i64,
    pub content_base64: String,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub expires_at: Option<DateTime<Utc>>,
}

pub async fn list_versions(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<SecureScriptVersionsResponse>>, AppError> {
    ensure_active_entitlement(&client)?;

    let scripts = SecureScriptRepository::new(state.db.clone())
        .list_published(client.tenant_id, client.app_id)
        .await?;
    let items = scripts
        .into_iter()
        .filter(|script| {
            ensure_required_features(&client.features, &script.required_features).is_ok()
        })
        .map(SecureScriptVersionSummary::from)
        .collect();

    Ok(Json(ApiResponse::ok(
        SecureScriptVersionsResponse { items },
        request_id.to_string(),
    )))
}

pub async fn fetch_script(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<FetchSecureScriptInput>,
) -> Result<Json<ApiResponse<FetchSecureScriptResponse>>, AppError> {
    rate_limit::check_client_action(&state, "script_fetch", &client.device_id.to_string()).await?;
    ensure_active_entitlement(&client)?;

    let script = SecureScriptRepository::new(state.db.clone())
        .find_by_id(client.tenant_id, payload.script_id)
        .await?
        .ok_or_else(AppError::script_not_found)?;
    validate_fetchable_script(&client, &script)?;
    let content = decrypt_script_content(&state, &script)?;

    Ok(Json(ApiResponse::ok(
        FetchSecureScriptResponse {
            script_id: script.id,
            version: script.version,
            version_code: script.version_code,
            content_base64: STANDARD.encode(content),
            sha256: script.content_sha256,
            signature_kid: script.signature_kid,
            signature: script.signature,
            signature_alg: script.signature_alg,
            expires_at: script.expires_at,
        },
        request_id.to_string(),
    )))
}

fn validate_fetchable_script(
    client: &ClientContext,
    script: &SecureScript,
) -> Result<(), AppError> {
    if script.app_id != client.app_id {
        return Err(AppError::script_not_found());
    }
    if script.status != "published" {
        return Err(AppError::script_not_found());
    }
    if let Some(expires_at) = script.expires_at {
        if expires_at <= Utc::now() {
            return Err(AppError::script_not_found());
        }
    }

    ensure_required_features(&client.features, &script.required_features)
}

fn decrypt_script_content(state: &AppState, script: &SecureScript) -> Result<Vec<u8>, AppError> {
    let envelope: PrivateKeyEnvelope = serde_json::from_str(&script.content_ciphertext)
        .map_err(|error| AppError::crypto(format!("script envelope invalid: {error}")))?;

    decrypt_bytes(&state.config.security.master_key, &envelope)
}

#[cfg(test)]
mod tests {
    use crate::modules::{client_auth::session::ClientContext, secure_script::model::SecureScript};
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use super::validate_fetchable_script;

    #[test]
    fn draft_script_is_not_fetchable() {
        let client = fixture_client(json!(["script"]));
        let mut script = fixture_script("draft", json!(["script"]));

        assert!(validate_fetchable_script(&client, &script).is_err());
        script.status = "published".to_owned();
        assert!(validate_fetchable_script(&client, &script).is_ok());
    }

    #[test]
    fn missing_required_feature_rejects_fetch() {
        let client = fixture_client(json!(["basic"]));
        let script = fixture_script("published", json!(["script"]));

        assert!(validate_fetchable_script(&client, &script).is_err());
    }

    fn fixture_client(features: serde_json::Value) -> ClientContext {
        ClientContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: None,
            device_id: Uuid::nil(),
            machine_id: "machine".to_owned(),
            auth_mode: "license".to_owned(),
            entitlement_id: Some(Uuid::nil()),
            entitlement_kind: Some("license".to_owned()),
            entitlement_status: "active".to_owned(),
            entitlement_active: true,
            features,
            entitlement_expires_at: None,
        }
    }

    fn fixture_script(status: &str, required_features: serde_json::Value) -> SecureScript {
        SecureScript {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            name: "script".to_owned(),
            version: "1.0.0".to_owned(),
            version_code: 100,
            status: status.to_owned(),
            content_ciphertext: "{}".to_owned(),
            content_sha256: "a".repeat(64),
            signing_key_id: Uuid::nil(),
            signature_kid: "kid".to_owned(),
            signature: "sig".to_owned(),
            signature_alg: "Ed25519".to_owned(),
            required_features,
            expires_at: None,
            published_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }
}
