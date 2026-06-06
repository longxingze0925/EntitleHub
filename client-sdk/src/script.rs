use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    jwks::{require_eddsa_public_key, JwksCache, JwksKey},
    signing::verify_ed25519_signature,
    SdkError, SdkResult,
};

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptPackage {
    pub script_id: String,
    pub version: String,
    pub version_code: i64,
    pub content_base64: String,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub expires_at_unix: Option<i64>,
}

impl ScriptPackage {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let package: Self = serde_json::from_str(json)
            .map_err(|_| SdkError::InvalidUpdateInfo("script_package"))?;
        validate_script_package(&package)?;

        Ok(package)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        let package: Self = crate::response::parse_api_response_data(json)?.data;
        validate_script_package(&package)?;

        Ok(package)
    }
}

pub fn verify_and_decode_script(package: &ScriptPackage, jwks: &[JwksKey]) -> SdkResult<Vec<u8>> {
    validate_script_package(package)?;
    if let Some(expires_at) = package.expires_at_unix {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SdkError::ExpiredScript)?
            .as_secs() as i64;
        if expires_at <= now {
            return Err(SdkError::ExpiredScript);
        }
    }

    verify_script_signature(package, jwks)?;
    let content = STANDARD
        .decode(package.content_base64.trim())
        .map_err(|_| SdkError::Base64Invalid)?;
    let actual_hash = format!("{:x}", Sha256::digest(&content));
    if actual_hash != package.sha256.to_lowercase() {
        return Err(SdkError::HashMismatch);
    }

    Ok(content)
}

pub fn verify_and_decode_script_with_jwks_refresh<F>(
    package: &ScriptPackage,
    jwks_cache: &mut JwksCache,
    fetch_jwks_json: F,
) -> SdkResult<Vec<u8>>
where
    F: FnMut() -> SdkResult<String>,
{
    validate_script_package(package)?;
    if let Some(expires_at) = package.expires_at_unix {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SdkError::ExpiredScript)?
            .as_secs() as i64;
        if expires_at <= now {
            return Err(SdkError::ExpiredScript);
        }
    }

    verify_script_signature_with_jwks_refresh(package, jwks_cache, fetch_jwks_json)?;
    let content = STANDARD
        .decode(package.content_base64.trim())
        .map_err(|_| SdkError::Base64Invalid)?;
    let actual_hash = format!("{:x}", Sha256::digest(&content));
    if actual_hash != package.sha256.to_lowercase() {
        return Err(SdkError::HashMismatch);
    }

    Ok(content)
}

pub fn verify_script_signature(package: &ScriptPackage, jwks: &[JwksKey]) -> SdkResult<()> {
    validate_script_package(package)?;
    let public_key_pem = require_eddsa_public_key(jwks, &package.signature_kid)?;
    verify_script_signature_with_public_key(package, public_key_pem)
}

pub fn verify_script_signature_with_jwks_refresh<F>(
    package: &ScriptPackage,
    jwks_cache: &mut JwksCache,
    fetch_jwks_json: F,
) -> SdkResult<()>
where
    F: FnMut() -> SdkResult<String>,
{
    validate_script_package(package)?;
    let public_key_pem = jwks_cache
        .require_eddsa_public_key_with_refresh(&package.signature_kid, fetch_jwks_json)?;
    verify_script_signature_with_public_key(package, public_key_pem)
}

fn verify_script_signature_with_public_key(
    package: &ScriptPackage,
    public_key_pem: &str,
) -> SdkResult<()> {
    let payload =
        secure_script_signature_payload(&package.sha256.to_lowercase(), package.version_code);
    verify_ed25519_signature(public_key_pem, payload.as_bytes(), &package.signature)
}

pub fn secure_script_signature_payload(content_sha256: &str, version_code: i64) -> String {
    format!("{content_sha256}:{version_code}")
}

fn validate_script_package(package: &ScriptPackage) -> SdkResult<()> {
    if package.version_code <= 0 {
        return Err(SdkError::InvalidUpdateInfo("version_code"));
    }
    if package.signature_alg != "Ed25519" {
        return Err(SdkError::UnsupportedSignatureAlg(
            package.signature_alg.clone(),
        ));
    }
    if package.signature.trim().is_empty() {
        return Err(SdkError::InvalidUpdateInfo("signature"));
    }
    if package.signature_kid.trim().is_empty() {
        return Err(SdkError::InvalidUpdateInfo("signature_kid"));
    }
    let sha256 = package.sha256.trim();
    if sha256.len() != 64 || !sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(SdkError::InvalidUpdateInfo("sha256"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::jwks::{JwksCache, JwksKey};

    use super::{
        secure_script_signature_payload, verify_and_decode_script,
        verify_and_decode_script_with_jwks_refresh, ScriptPackage,
    };

    #[test]
    fn verify_and_decode_script_accepts_valid_package() {
        let (package, jwks, content) = fixture_script_package(b"print('ok')");

        let decoded =
            verify_and_decode_script(&package, &jwks).expect("script should verify and decode");

        assert_eq!(decoded, content);
    }

    #[test]
    fn verify_and_decode_script_rejects_tampered_content() {
        let (mut package, jwks, _content) = fixture_script_package(b"print('ok')");
        package.content_base64 = base64::engine::general_purpose::STANDARD.encode(b"tampered");

        let error =
            verify_and_decode_script(&package, &jwks).expect_err("tampered script should fail");

        assert!(matches!(error, crate::SdkError::HashMismatch));
    }

    #[test]
    fn verify_and_decode_script_rejects_missing_signature() {
        let (mut package, jwks, _content) = fixture_script_package(b"print('ok')");
        package.signature.clear();

        let error =
            verify_and_decode_script(&package, &jwks).expect_err("missing signature should fail");

        assert!(matches!(
            error,
            crate::SdkError::InvalidUpdateInfo("signature")
        ));
    }

    #[test]
    fn verify_and_decode_script_refreshes_missing_jwks_key() {
        let (package, _jwks, content, jwks_json) =
            fixture_script_package_with_jwks_json(b"print('ok')");
        let mut cache = JwksCache::new();

        let decoded = verify_and_decode_script_with_jwks_refresh(&package, &mut cache, || {
            Ok(jwks_json.clone())
        })
        .expect("script should verify after jwks refresh");

        assert_eq!(decoded, content);
        assert!(cache.find("kid").is_some());
    }

    #[test]
    fn verify_and_decode_script_rejects_expired_script() {
        let (mut package, jwks, _content) = fixture_script_package(b"print('ok')");
        package.expires_at_unix = Some(now_unix() - 1);

        let error =
            verify_and_decode_script(&package, &jwks).expect_err("expired script should fail");

        assert!(matches!(error, crate::SdkError::ExpiredScript));
    }

    #[test]
    fn script_package_parses_api_response_wrapper_and_validates_shape() {
        let (package, _jwks, _content) = fixture_script_package(b"print('ok')");
        let json = serde_json::json!({
            "code": 0,
            "message": "ok",
            "data": {
                "script_id": package.script_id,
                "version": package.version,
                "version_code": package.version_code,
                "content_base64": package.content_base64,
                "sha256": package.sha256,
                "signature_kid": package.signature_kid,
                "signature": package.signature,
                "signature_alg": package.signature_alg,
                "expires_at_unix": package.expires_at_unix
            },
            "request_id": "req_1"
        })
        .to_string();

        let parsed =
            ScriptPackage::from_api_response_json(&json).expect("api response should parse");

        assert_eq!(parsed.version_code, package.version_code);
        assert!(ScriptPackage::from_json(r#"{"version_code": 0}"#).is_err());
    }

    fn fixture_script_package(content: &[u8]) -> (ScriptPackage, Vec<JwksKey>, Vec<u8>) {
        let (package, jwks, content, _jwks_json) = fixture_script_package_with_jwks_json(content);

        (package, jwks, content)
    }

    fn fixture_script_package_with_jwks_json(
        content: &[u8],
    ) -> (ScriptPackage, Vec<JwksKey>, Vec<u8>, String) {
        let sha256 = format!("{:x}", Sha256::digest(content));
        let version_code = 100;
        let key_pair = generate_key_pair();
        let raw_public_key = key_pair.public_key().as_ref();
        let public_key_pem = public_key_pem(raw_public_key);
        let payload = secure_script_signature_payload(&sha256, version_code);
        let signature = URL_SAFE_NO_PAD.encode(key_pair.sign(payload.as_bytes()).as_ref());
        let package = ScriptPackage {
            script_id: "00000000-0000-0000-0000-000000000000".to_owned(),
            version: "1.0.0".to_owned(),
            version_code,
            content_base64: base64::engine::general_purpose::STANDARD.encode(content),
            sha256,
            signature_kid: "kid".to_owned(),
            signature,
            signature_alg: "Ed25519".to_owned(),
            expires_at_unix: Some(now_unix() + 300),
        };
        let jwks = vec![JwksKey {
            kid: "kid".to_owned(),
            alg: "EdDSA".to_owned(),
            public_key_pem,
        }];
        let jwks_json = jwks_json("kid", raw_public_key);

        (package, jwks, content.to_vec(), jwks_json)
    }

    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_secs() as i64
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
