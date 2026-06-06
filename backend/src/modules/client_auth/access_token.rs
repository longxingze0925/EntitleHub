use chrono::Utc;

use crate::{
    crypto::{
        envelope::{decrypt_private_key, encrypt_private_key, PrivateKeyEnvelope},
        jwt::{decode_eddsa_header, sign_eddsa, verify_eddsa, ClientAccessClaims},
        signing::generate_ed25519_key,
    },
    error::AppError,
    http::request_id::RequestId,
    modules::application::{
        model::{NewSigningKey, SigningKey},
        repository::{create_signing_key_in_transaction, ApplicationRepository},
    },
    modules::audit::{self, AuditLogInput},
    state::AppState,
};

const JWT_ACCESS_TOKEN_KEY_SCOPE: &str = "jwt_access_token";

pub async fn sign_client_access_token(
    state: &AppState,
    claims: &ClientAccessClaims,
    request_id: Option<&RequestId>,
) -> Result<String, AppError> {
    let signer = prepare_client_access_token_signer(state, request_id).await?;

    signer.sign(claims)
}

#[derive(Debug, Clone)]
pub struct ClientAccessTokenSigner {
    kid: String,
    private_key: Vec<u8>,
}

impl ClientAccessTokenSigner {
    pub fn sign(&self, claims: &ClientAccessClaims) -> Result<String, AppError> {
        sign_eddsa(claims, &self.kid, &self.private_key)
    }
}

pub async fn prepare_client_access_token_signer(
    state: &AppState,
    request_id: Option<&RequestId>,
) -> Result<ClientAccessTokenSigner, AppError> {
    let signing_key = get_or_create_jwt_signing_key(state, request_id).await?;
    let private_key = decrypt_signing_key(state, &signing_key)?;

    Ok(ClientAccessTokenSigner {
        kid: signing_key.kid,
        private_key,
    })
}

pub async fn verify_client_access_token(
    state: &AppState,
    token: &str,
) -> Result<ClientAccessClaims, AppError> {
    let header = decode_eddsa_header(token)?;
    let signing_key = ApplicationRepository::new(state.db.clone())
        .find_public_global_signing_key_by_kid(JWT_ACCESS_TOKEN_KEY_SCOPE, &header.kid)
        .await?
        .ok_or_else(|| AppError::token_invalid("access token invalid"))?;

    verify_eddsa(
        token,
        &signing_key.public_key_pem,
        &state.config.security.jwt_issuer,
        &state.config.security.jwt_audience,
        Utc::now().timestamp(),
    )
}

async fn get_or_create_jwt_signing_key(
    state: &AppState,
    request_id: Option<&RequestId>,
) -> Result<SigningKey, AppError> {
    let repository = ApplicationRepository::new(state.db.clone());
    if let Some(signing_key) = repository
        .find_active_global_signing_key_with_private_envelope(JWT_ACCESS_TOKEN_KEY_SCOPE)
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
    let new_signing_key = NewSigningKey::jwt_access_token(
        generated_signing_key.kid,
        generated_signing_key.public_key_pem,
        private_key_envelope_json,
    );

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let signing_key = create_signing_key_in_transaction(&mut transaction, new_signing_key).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: None,
            actor_type: "system",
            actor_id: None,
            action: "jwt.signing_key.create",
            resource_type: "signing_key",
            resource_id: Some(signing_key.id),
            ip: None,
            user_agent: None,
            request_id: request_id.map(ToString::to_string),
            before_json: None,
            after_json: Some(serde_json::json!({
                "id": signing_key.id,
                "kid": signing_key.kid,
                "key_scope": signing_key.key_scope,
                "status": signing_key.status,
            })),
            metadata_json: serde_json::json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(signing_key)
}

fn decrypt_signing_key(state: &AppState, signing_key: &SigningKey) -> Result<Vec<u8>, AppError> {
    let private_key_envelope = signing_key
        .private_key_envelope
        .clone()
        .ok_or_else(|| AppError::crypto("jwt signing key private envelope missing"))?;
    let private_key_envelope: PrivateKeyEnvelope = serde_json::from_value(private_key_envelope)
        .map_err(|error| AppError::crypto(format!("jwt signing key envelope invalid: {error}")))?;

    decrypt_private_key(&state.config.security.master_key, &private_key_envelope)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("client access token database error: {error}"))
}
