use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Deserialize;

use crate::{
    jwks::{require_eddsa_public_key, JwksCache, JwksKey},
    signing::verify_ed25519_signature,
    SdkError, SdkResult,
};

#[derive(Debug, Clone, Deserialize)]
pub struct AccessTokenHeader {
    pub alg: String,
    pub kid: String,
    pub typ: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientAccessClaims {
    pub sub: String,
    pub iss: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    pub session_id: String,
    pub tenant_id: String,
    pub app_id: String,
    pub device_id: String,
    pub machine_id: String,
    pub auth_mode: String,
}

pub fn decode_access_token_header(token: &str) -> SdkResult<AccessTokenHeader> {
    let parts = token_parts(token)?;
    let header_bytes = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| SdkError::InvalidAccessToken)?;
    let header: AccessTokenHeader =
        serde_json::from_slice(&header_bytes).map_err(|_| SdkError::InvalidAccessToken)?;
    if header.alg != "EdDSA" || header.kid.trim().is_empty() {
        return Err(SdkError::InvalidAccessToken);
    }

    Ok(header)
}

pub fn verify_access_token(
    token: &str,
    jwks: &[JwksKey],
    expected_issuer: &str,
    expected_audience: &str,
    now_unix: i64,
) -> SdkResult<ClientAccessClaims> {
    let header = decode_access_token_header(token)?;
    let public_key_pem = require_eddsa_public_key(jwks, &header.kid)?;

    verify_access_token_with_public_key(
        token,
        public_key_pem,
        expected_issuer,
        expected_audience,
        now_unix,
    )
}

pub fn verify_access_token_with_jwks_refresh<F>(
    token: &str,
    jwks_cache: &mut JwksCache,
    expected_issuer: &str,
    expected_audience: &str,
    now_unix: i64,
    fetch_jwks_json: F,
) -> SdkResult<ClientAccessClaims>
where
    F: FnMut() -> SdkResult<String>,
{
    let header = decode_access_token_header(token)?;
    let public_key_pem =
        jwks_cache.require_eddsa_public_key_with_refresh(&header.kid, fetch_jwks_json)?;

    verify_access_token_with_public_key(
        token,
        public_key_pem,
        expected_issuer,
        expected_audience,
        now_unix,
    )
}

fn verify_access_token_with_public_key(
    token: &str,
    public_key_pem: &str,
    expected_issuer: &str,
    expected_audience: &str,
    now_unix: i64,
) -> SdkResult<ClientAccessClaims> {
    let parts = token_parts(token)?;
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    verify_ed25519_signature(public_key_pem, signing_input.as_bytes(), parts[2])
        .map_err(|_| SdkError::InvalidAccessToken)?;

    let claims_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| SdkError::InvalidAccessToken)?;
    let claims: ClientAccessClaims =
        serde_json::from_slice(&claims_bytes).map_err(|_| SdkError::InvalidAccessToken)?;

    if claims.iss != expected_issuer || claims.aud != expected_audience {
        return Err(SdkError::InvalidAccessToken);
    }
    if claims.exp <= now_unix {
        return Err(SdkError::ExpiredAccessToken);
    }

    Ok(claims)
}

fn token_parts(token: &str) -> SdkResult<Vec<&str>> {
    let parts = token.split('.').collect::<Vec<_>>();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
        return Err(SdkError::InvalidAccessToken);
    }

    Ok(parts)
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use serde_json::json;

    use crate::jwks::{JwksCache, JwksKey};

    use super::{
        decode_access_token_header, verify_access_token, verify_access_token_with_jwks_refresh,
    };

    #[test]
    fn verify_access_token_accepts_valid_eddsa_jwt() {
        let (token, jwks, _jwks_json) = fixture_access_token(200);

        let claims = verify_access_token(&token, &jwks, "issuer", "audience", 100)
            .expect("access token should verify");

        assert_eq!(claims.session_id, "session-id");
        let header = decode_access_token_header(&token).expect("header should decode");
        assert_eq!(header.kid, "kid");
    }

    #[test]
    fn verify_access_token_rejects_expired_token() {
        let (token, jwks, _jwks_json) = fixture_access_token(100);

        let error = verify_access_token(&token, &jwks, "issuer", "audience", 100)
            .expect_err("expired access token should fail");

        assert!(matches!(error, crate::SdkError::ExpiredAccessToken));
    }

    #[test]
    fn verify_access_token_refreshes_missing_jwks_key() {
        let (token, _jwks, jwks_json) = fixture_access_token(200);
        let mut cache = JwksCache::new();

        let claims = verify_access_token_with_jwks_refresh(
            &token,
            &mut cache,
            "issuer",
            "audience",
            100,
            || Ok(jwks_json.clone()),
        )
        .expect("access token should verify after jwks refresh");

        assert_eq!(claims.device_id, "device-id");
        assert!(cache.find("kid").is_some());
    }

    fn fixture_access_token(expires_at: i64) -> (String, Vec<JwksKey>, String) {
        let key_pair = generate_key_pair();
        let raw_public_key = key_pair.public_key().as_ref();
        let public_key_pem = public_key_pem(raw_public_key);
        let header = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "alg": "EdDSA",
                "typ": "JWT",
                "kid": "kid"
            }))
            .expect("header json"),
        );
        let claims = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "sub": "session-id",
                "iss": "issuer",
                "aud": "audience",
                "exp": expires_at,
                "iat": 1,
                "session_id": "session-id",
                "tenant_id": "tenant-id",
                "app_id": "app-id",
                "device_id": "device-id",
                "machine_id": "machine-id",
                "auth_mode": "license"
            }))
            .expect("claims json"),
        );
        let signing_input = format!("{header}.{claims}");
        let signature = URL_SAFE_NO_PAD.encode(key_pair.sign(signing_input.as_bytes()).as_ref());
        let token = format!("{signing_input}.{signature}");
        let jwks = vec![JwksKey {
            kid: "kid".to_owned(),
            alg: "EdDSA".to_owned(),
            public_key_pem,
        }];
        let jwks_json = jwks_json("kid", raw_public_key);

        (token, jwks, jwks_json)
    }

    fn generate_key_pair() -> Ed25519KeyPair {
        let random = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&random).expect("generate key");
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("read key")
    }

    fn public_key_pem(raw_public_key: &[u8]) -> String {
        let mut der = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        der.extend_from_slice(raw_public_key);
        let encoded = base64::engine::general_purpose::STANDARD.encode(der);
        format!("-----BEGIN PUBLIC KEY-----\n{encoded}\n-----END PUBLIC KEY-----\n")
    }

    fn jwks_json(kid: &str, raw_public_key: &[u8]) -> String {
        format!(
            r#"{{
              "keys": [{{
                "kid": "{kid}",
                "kty": "OKP",
                "crv": "Ed25519",
                "alg": "EdDSA",
                "use": "sig",
                "x": "{}"
              }}]
            }}"#,
            URL_SAFE_NO_PAD.encode(raw_public_key)
        )
    }
}
