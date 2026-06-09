use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Deserializer, Serialize};
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
    #[serde(
        default,
        alias = "expires_at",
        deserialize_with = "deserialize_optional_expires_at_unix"
    )]
    pub expires_at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FetchSecureScriptRequestPayload {
    pub script_id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SecureScriptVersionSummary {
    pub script_id: String,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SecureScriptVersionsResponse {
    pub items: Vec<SecureScriptVersionSummary>,
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

impl SecureScriptVersionsResponse {
    pub fn from_json(json: &str) -> SdkResult<Self> {
        let response: Self = serde_json::from_str(json)
            .map_err(|_| SdkError::InvalidUpdateInfo("script_versions"))?;
        validate_script_versions_response(&response)?;

        Ok(response)
    }

    pub fn from_api_response_json(json: &str) -> SdkResult<Self> {
        let response: Self = crate::response::parse_api_response_data(json)?.data;
        validate_script_versions_response(&response)?;

        Ok(response)
    }
}

pub fn build_fetch_secure_script_request(
    script_id: &str,
) -> SdkResult<FetchSecureScriptRequestPayload> {
    Ok(FetchSecureScriptRequestPayload {
        script_id: clean_required("script_id", script_id)?,
    })
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

fn validate_script_versions_response(response: &SecureScriptVersionsResponse) -> SdkResult<()> {
    for item in &response.items {
        if item.script_id.trim().is_empty()
            || item.name.trim().is_empty()
            || item.version.trim().is_empty()
            || item.version_code <= 0
            || item.sha256.len() != 64
            || !item.sha256.chars().all(|ch| ch.is_ascii_hexdigit())
            || item.signature_kid.trim().is_empty()
            || item.signature.trim().is_empty()
            || item.signature_alg != "Ed25519"
        {
            return Err(SdkError::InvalidUpdateInfo("script_versions"));
        }
    }

    Ok(())
}

fn clean_required(field: &'static str, value: &str) -> SdkResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(SdkError::InvalidClientRequest(field));
    }

    Ok(value.to_owned())
}

fn deserialize_optional_expires_at_unix<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let Some(value) = Option::<serde_json::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };

    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(number) => number
            .as_i64()
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expires_at_unix must be positive")),
        serde_json::Value::String(value) => {
            let value = value.trim();
            if value.is_empty() {
                return Ok(None);
            }
            if let Ok(unix) = value.parse::<i64>() {
                if unix > 0 {
                    return Ok(Some(unix));
                }
            }
            parse_rfc3339_to_unix(value)
                .filter(|unix| *unix > 0)
                .map(Some)
                .ok_or_else(|| serde::de::Error::custom("expires_at must be RFC3339"))
        }
        _ => Err(serde::de::Error::custom(
            "expires_at must be string, number, or null",
        )),
    }
}

fn parse_rfc3339_to_unix(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let year = parse_fixed_i32(value, 0, 4)?;
    expect_byte(bytes, 4, b'-')?;
    let month = parse_fixed_u32(value, 5, 2)?;
    expect_byte(bytes, 7, b'-')?;
    let day = parse_fixed_u32(value, 8, 2)?;
    if !matches!(bytes.get(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let hour = parse_fixed_u32(value, 11, 2)?;
    expect_byte(bytes, 13, b':')?;
    let minute = parse_fixed_u32(value, 14, 2)?;
    expect_byte(bytes, 16, b':')?;
    let second = parse_fixed_u32(value, 17, 2)?;
    if month == 0
        || month > 12
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        let start = index;
        while matches!(bytes.get(index), Some(byte) if byte.is_ascii_digit()) {
            index += 1;
        }
        if index == start {
            return None;
        }
    }

    let offset_seconds = match bytes.get(index).copied()? {
        b'Z' | b'z' => {
            index += 1;
            0
        }
        b'+' | b'-' => {
            let sign = if bytes[index] == b'+' { 1 } else { -1 };
            let offset_hour = parse_fixed_u32(value, index + 1, 2)?;
            expect_byte(bytes, index + 3, b':')?;
            let offset_minute = parse_fixed_u32(value, index + 4, 2)?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            index += 6;
            sign * ((offset_hour as i64 * 3600) + (offset_minute as i64 * 60))
        }
        _ => return None,
    };
    if index != bytes.len() {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64 - offset_seconds)
}

fn parse_fixed_i32(value: &str, start: usize, len: usize) -> Option<i32> {
    value.get(start..start + len)?.parse().ok()
}

fn parse_fixed_u32(value: &str, start: usize, len: usize) -> Option<u32> {
    let part = value.get(start..start + len)?;
    part.chars()
        .all(|ch| ch.is_ascii_digit())
        .then(|| part.parse().ok())?
}

fn expect_byte(bytes: &[u8], index: usize, expected: u8) -> Option<()> {
    (bytes.get(index) == Some(&expected)).then_some(())
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year as i64 - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month as i64 + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    era * 146_097 + day_of_era - 719_468
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
        build_fetch_secure_script_request, parse_rfc3339_to_unix, secure_script_signature_payload,
        verify_and_decode_script, verify_and_decode_script_with_jwks_refresh, ScriptPackage,
        SecureScriptVersionsResponse,
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

    #[test]
    fn script_package_accepts_backend_expires_at_alias() {
        let (package, _jwks, _content) = fixture_script_package(b"print('ok')");
        let expires_at = "2027-01-01T00:00:00Z";
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
                "expires_at": expires_at
            },
            "request_id": "req_1"
        })
        .to_string();

        let parsed =
            ScriptPackage::from_api_response_json(&json).expect("api response should parse");

        assert_eq!(parsed.expires_at_unix, Some(1_798_761_600));
        assert_eq!(
            parse_rfc3339_to_unix("2027-01-01T08:00:00+08:00"),
            Some(1_798_761_600)
        );
    }

    #[test]
    fn script_versions_and_fetch_payloads_parse() {
        let payload = build_fetch_secure_script_request(" 00000000-0000-0000-0000-000000000010 ")
            .expect("fetch payload");
        assert_eq!(payload.script_id, "00000000-0000-0000-0000-000000000010");

        let versions = SecureScriptVersionsResponse::from_api_response_json(
            r#"{
              "code": 0,
              "message": "ok",
              "data": {
                "items": [{
                  "script_id": "00000000-0000-0000-0000-000000000010",
                  "name": "loader",
                  "version": "1.0.0",
                  "version_code": 100,
                  "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                  "signature_kid": "script-kid",
                  "signature": "signature",
                  "signature_alg": "Ed25519",
                  "expires_at": null
                }]
              },
              "request_id": "req_1"
            }"#,
        )
        .expect("versions should parse");

        assert_eq!(versions.items.len(), 1);
        assert_eq!(versions.items[0].name, "loader");
        assert!(build_fetch_secure_script_request(" ").is_err());
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
