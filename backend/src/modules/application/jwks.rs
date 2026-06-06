use axum::{
    extract::{Path, State},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;

use crate::{
    crypto::signing::parse_ed25519_public_key,
    error::AppError,
    modules::application::{model::SigningKey, repository::ApplicationRepository},
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct JwksResponse {
    pub keys: Vec<JwkKey>,
}

#[derive(Debug, Serialize)]
pub struct JwkKey {
    pub kid: String,
    pub kty: &'static str,
    pub crv: &'static str,
    pub alg: String,
    #[serde(rename = "use")]
    pub key_use: &'static str,
    pub x: String,
}

pub async fn global_jwks(State(state): State<AppState>) -> Result<Json<JwksResponse>, AppError> {
    let keys = ApplicationRepository::new(state.db.clone())
        .list_global_public_jwks_keys()
        .await?;

    Ok(Json(JwksResponse {
        keys: jwk_keys(keys)?,
    }))
}

pub async fn application_jwks(
    State(state): State<AppState>,
    Path(app_key): Path<String>,
) -> Result<Json<JwksResponse>, AppError> {
    let keys = ApplicationRepository::new(state.db.clone())
        .list_public_jwks_keys_for_app_key(app_key.trim())
        .await?;

    Ok(Json(JwksResponse {
        keys: jwk_keys(keys)?,
    }))
}

fn jwk_keys(keys: Vec<SigningKey>) -> Result<Vec<JwkKey>, AppError> {
    keys.into_iter().map(jwk_key).collect()
}

fn jwk_key(key: SigningKey) -> Result<JwkKey, AppError> {
    let raw_public_key = parse_ed25519_public_key(&key.public_key_pem)
        .map_err(|_| AppError::crypto("signing key public key is invalid"))?;

    Ok(JwkKey {
        kid: key.kid,
        kty: "OKP",
        crv: "Ed25519",
        alg: key.alg,
        key_use: "sig",
        x: URL_SAFE_NO_PAD.encode(raw_public_key),
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::{
        crypto::signing::generate_ed25519_key,
        modules::application::{jwks::jwk_key, model::SigningKey},
    };

    #[test]
    fn jwk_key_contains_only_public_jwk_fields() {
        let generated = generate_ed25519_key().expect("key should generate");
        let jwk = jwk_key(SigningKey {
            id: Uuid::nil(),
            tenant_id: Some(Uuid::nil()),
            app_id: Some(Uuid::nil()),
            key_scope: "app_request".to_owned(),
            kid: "kid".to_owned(),
            alg: "EdDSA".to_owned(),
            public_key_pem: generated.public_key_pem,
            private_key_envelope: None,
            status: "active".to_owned(),
            not_before: Utc::now(),
            not_after: None,
            rotated_from_id: None,
            created_by: None,
            created_at: Utc::now(),
            activated_at: Some(Utc::now()),
            retired_at: None,
            revoked_at: None,
        })
        .expect("jwk should convert");

        assert_eq!(jwk.kty, "OKP");
        assert_eq!(jwk.crv, "Ed25519");
        assert_eq!(jwk.alg, "EdDSA");
        assert!(!jwk.x.is_empty());
    }
}
