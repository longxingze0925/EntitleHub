use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::{
    crypto::signing::{sign_ed25519, verify_ed25519_signature},
    error::AppError,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClientAccessClaims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    pub session_id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub device_id: Uuid,
    pub machine_id: String,
    pub auth_mode: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JwtHeader {
    pub alg: String,
    pub typ: Option<String>,
    pub kid: String,
}

pub fn sign_eddsa(
    claims: &ClientAccessClaims,
    kid: &str,
    private_key_pkcs8_der: &[u8],
) -> Result<String, AppError> {
    let kid = kid.trim();
    if kid.is_empty() {
        return Err(AppError::crypto("jwt kid is required"));
    }
    let header = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({
            "alg": "EdDSA",
            "typ": "JWT",
            "kid": kid,
        }))
        .map_err(|error| AppError::crypto(format!("jwt header serialization failed: {error}")))?,
    );
    let payload =
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).map_err(|error| {
            AppError::crypto(format!("jwt claims serialization failed: {error}"))
        })?);
    let signing_input = format!("{header}.{payload}");
    let signature = sign_ed25519(private_key_pkcs8_der, signing_input.as_bytes())?;

    Ok(format!("{signing_input}.{signature}"))
}

pub fn decode_eddsa_header(token: &str) -> Result<JwtHeader, AppError> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(AppError::token_invalid("access token invalid"));
    }
    let header_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| AppError::token_invalid("access token invalid"))?;
    let header = serde_json::from_slice::<JwtHeader>(&header_bytes)
        .map_err(|_| AppError::token_invalid("access token invalid"))?;
    if header.alg != "EdDSA" || header.kid.trim().is_empty() {
        return Err(AppError::token_invalid("access token invalid"));
    }

    Ok(header)
}

pub fn verify_eddsa(
    token: &str,
    public_key_pem: &str,
    issuer: &str,
    audience: &str,
    now: i64,
) -> Result<ClientAccessClaims, AppError> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(AppError::token_invalid("access token invalid"));
    }
    decode_eddsa_header(token)?;

    let signing_input = format!("{}.{}", parts[0], parts[1]);
    verify_ed25519_signature(public_key_pem, signing_input.as_bytes(), parts[2])
        .map_err(|_| AppError::token_invalid("access token invalid"))?;

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| AppError::token_invalid("access token invalid"))?;
    let claims = serde_json::from_slice::<ClientAccessClaims>(&claims_bytes)
        .map_err(|_| AppError::token_invalid("access token invalid"))?;

    if claims.iss != issuer || claims.aud != audience {
        return Err(AppError::token_invalid("access token invalid"));
    }

    if claims.exp <= now {
        return Err(AppError::token_expired());
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::error::AppError;

    use super::{decode_eddsa_header, sign_eddsa, verify_eddsa, ClientAccessClaims};
    use crate::crypto::signing::generate_ed25519_key;

    #[test]
    fn jwt_has_three_segments() {
        let key = generate_ed25519_key().expect("key should generate");
        let token = sign_eddsa(&claims(100), &key.kid, &key.private_key_pkcs8_der)
            .expect("jwt should sign");

        assert_eq!(token.split('.').count(), 3);
        let header = decode_eddsa_header(&token).expect("jwt header should decode");
        assert_eq!(header.alg, "EdDSA");
        assert_eq!(header.kid, key.kid);
    }

    #[test]
    fn jwt_verifies_signature_and_claims() {
        let key = generate_ed25519_key().expect("key should generate");
        let token = sign_eddsa(&claims(100), &key.kid, &key.private_key_pkcs8_der)
            .expect("jwt should sign");
        let verified = verify_eddsa(&token, &key.public_key_pem, "issuer", "audience", 99)
            .expect("jwt should verify");

        assert_eq!(verified.session_id, Uuid::nil());
    }

    #[test]
    fn jwt_rejects_expired_token() {
        let key = generate_ed25519_key().expect("key should generate");
        let token = sign_eddsa(&claims(100), &key.kid, &key.private_key_pkcs8_der)
            .expect("jwt should sign");

        assert!(matches!(
            verify_eddsa(&token, &key.public_key_pem, "issuer", "audience", 100),
            Err(AppError::TokenExpired(_))
        ));
    }

    #[test]
    fn jwt_rejects_invalid_token_with_stable_error_code() {
        assert!(matches!(
            verify_eddsa("invalid-token", "public-key", "issuer", "audience", 99),
            Err(AppError::TokenInvalid(_))
        ));
    }

    #[test]
    fn jwt_rejects_wrong_public_key() {
        let signing_key = generate_ed25519_key().expect("signing key should generate");
        let other_key = generate_ed25519_key().expect("other key should generate");
        let token = sign_eddsa(
            &claims(100),
            &signing_key.kid,
            &signing_key.private_key_pkcs8_der,
        )
        .expect("jwt should sign");

        assert!(matches!(
            verify_eddsa(&token, &other_key.public_key_pem, "issuer", "audience", 99),
            Err(AppError::TokenInvalid(_))
        ));
    }

    fn claims(exp: i64) -> ClientAccessClaims {
        ClientAccessClaims {
            sub: Uuid::nil().to_string(),
            iss: "issuer".to_owned(),
            aud: "audience".to_owned(),
            exp,
            iat: 1,
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            device_id: Uuid::nil(),
            machine_id: "machine".to_owned(),
            auth_mode: "license".to_owned(),
        }
    }
}
