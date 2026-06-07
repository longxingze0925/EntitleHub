use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;

const EMPTY_SCRIPT_CONTENT_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

#[derive(Debug, Clone, FromRow)]
pub struct SecureScript {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub status: String,
    pub content_ciphertext: String,
    pub content_sha256: String,
    pub signing_key_id: Uuid,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub required_features: Value,
    pub expires_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewSecureScript {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub content_ciphertext: String,
    pub content_sha256: String,
    pub signing_key_id: Uuid,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub required_features: Value,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct UpdateSecureScriptContent {
    pub version: Option<String>,
    pub version_code: Option<i64>,
    pub content_ciphertext: String,
    pub content_sha256: String,
    pub signing_key_id: Uuid,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSecureScriptInput {
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub required_features: Option<Value>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSecureScriptContentInput {
    pub content_base64: String,
    pub version: Option<String>,
    pub version_code: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct FetchSecureScriptInput {
    pub script_id: Uuid,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecureScriptListQuery {
    pub status: Option<String>,
    pub include_history: Option<bool>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SecureScriptListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct SecureScriptSummary {
    pub id: Uuid,
    pub app_id: Uuid,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub status: String,
    pub content_sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub required_features: Value,
    pub expires_at: Option<DateTime<Utc>>,
    pub published_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct SecureScriptVersionSummary {
    pub script_id: Uuid,
    pub name: String,
    pub version: String,
    pub version_code: i64,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl NewSecureScript {
    pub fn from_input(
        tenant_id: Uuid,
        app_id: Uuid,
        input: CreateSecureScriptInput,
        encrypted_empty_content: String,
        signing_key_id: Uuid,
        signature_kid: String,
        signature: String,
    ) -> Result<Self, AppError> {
        let name = clean_required(input.name, "name")?;
        let version = clean_required(input.version, "version")?;
        let version_code = validate_version_code(input.version_code)?;
        let required_features = normalize_required_features(input.required_features)?;

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id,
            name,
            version,
            version_code,
            content_ciphertext: encrypted_empty_content,
            content_sha256: EMPTY_SCRIPT_CONTENT_SHA256.to_owned(),
            signing_key_id,
            signature_kid,
            signature,
            signature_alg: "Ed25519".to_owned(),
            required_features,
            expires_at: input.expires_at,
        })
    }
}

impl UpdateSecureScriptContent {
    pub fn new(
        content_ciphertext: String,
        content_sha256: String,
        signing_key_id: Uuid,
        signature_kid: String,
        signature: String,
        input: UpdateSecureScriptContentInput,
    ) -> Result<Self, AppError> {
        let version = input
            .version
            .map(|value| clean_required(value, "version"))
            .transpose()?;
        let version_code = input.version_code.map(validate_version_code).transpose()?;

        Ok(Self {
            version,
            version_code,
            content_ciphertext,
            content_sha256: normalize_sha256(content_sha256)?,
            signing_key_id,
            signature_kid,
            signature,
            signature_alg: "Ed25519".to_owned(),
        })
    }
}

impl From<SecureScript> for SecureScriptSummary {
    fn from(script: SecureScript) -> Self {
        Self {
            id: script.id,
            app_id: script.app_id,
            name: script.name,
            version: script.version,
            version_code: script.version_code,
            status: script.status,
            content_sha256: script.content_sha256,
            signature_kid: script.signature_kid,
            signature: script.signature,
            signature_alg: script.signature_alg,
            required_features: script.required_features,
            expires_at: script.expires_at,
            published_at: script.published_at,
        }
    }
}

impl From<SecureScript> for SecureScriptVersionSummary {
    fn from(script: SecureScript) -> Self {
        Self {
            script_id: script.id,
            name: script.name,
            version: script.version,
            version_code: script.version_code,
            sha256: script.content_sha256,
            signature_kid: script.signature_kid,
            signature: script.signature,
            signature_alg: script.signature_alg,
            expires_at: script.expires_at,
        }
    }
}

pub fn secure_script_signature_payload(content_sha256: &str, version_code: i64) -> String {
    format!("{content_sha256}:{version_code}")
}

pub fn empty_script_sha256() -> &'static str {
    EMPTY_SCRIPT_CONTENT_SHA256
}

pub fn validate_create_secure_script_input(
    input: &CreateSecureScriptInput,
) -> Result<(), AppError> {
    clean_required(input.name.clone(), "name")?;
    clean_required(input.version.clone(), "version")?;
    validate_version_code(input.version_code)?;
    normalize_required_features(input.required_features.clone())?;

    Ok(())
}

pub fn validate_update_secure_script_content_input(
    input: &UpdateSecureScriptContentInput,
) -> Result<(), AppError> {
    if let Some(version) = &input.version {
        clean_required(version.clone(), "version")?;
    }
    if let Some(version_code) = input.version_code {
        validate_version_code(version_code)?;
    }

    Ok(())
}

pub fn validate_secure_script_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(status, "draft" | "published" | "deprecated") {
        return Ok(());
    }

    Err(AppError::validation_failed(
        "secure script status is invalid",
    ))
}

pub fn ensure_required_features(
    license_features: &Value,
    required_features: &Value,
) -> Result<(), AppError> {
    let required = required_features
        .as_array()
        .ok_or_else(|| AppError::validation_failed("required_features must be an array"))?;
    let license = license_features
        .as_array()
        .ok_or_else(|| AppError::license_invalid("license features must be an array"))?;

    let has_all = required.iter().all(|feature| license.contains(feature));
    if has_all {
        return Ok(());
    }

    Err(AppError::forbidden("required feature missing"))
}

fn clean_required(value: String, field: &'static str) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::validation_failed(format!("{field} is required")));
    }

    Ok(value)
}

fn validate_version_code(value: i64) -> Result<i64, AppError> {
    if value <= 0 {
        return Err(AppError::validation_failed(
            "version_code must be greater than 0",
        ));
    }

    Ok(value)
}

fn normalize_required_features(required_features: Option<Value>) -> Result<Value, AppError> {
    let value = required_features.unwrap_or_else(|| serde_json::json!([]));
    if value.is_array() {
        return Ok(value);
    }

    Err(AppError::validation_failed(
        "required_features must be an array",
    ))
}

fn normalize_sha256(sha256: String) -> Result<String, AppError> {
    let sha256 = sha256.trim().to_lowercase();
    if sha256.len() == 64 && sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ok(sha256);
    }

    Err(AppError::validation_failed(
        "content_sha256 must be a 64-character hex string",
    ))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{
        empty_script_sha256, ensure_required_features, secure_script_signature_payload,
        validate_create_secure_script_input, validate_secure_script_status_filter,
        validate_update_secure_script_content_input, CreateSecureScriptInput, NewSecureScript,
        UpdateSecureScriptContentInput,
    };

    #[test]
    fn new_secure_script_applies_empty_content_defaults() {
        let script = NewSecureScript::from_input(
            Uuid::nil(),
            Uuid::nil(),
            CreateSecureScriptInput {
                name: " init ".to_owned(),
                version: "1.0.0".to_owned(),
                version_code: 100,
                required_features: None,
                expires_at: None,
            },
            "encrypted".to_owned(),
            Uuid::nil(),
            "kid".to_owned(),
            "sig".to_owned(),
        )
        .expect("script should be valid");

        assert_eq!(script.name, "init");
        assert_eq!(script.content_sha256, empty_script_sha256());
        assert!(script.required_features.is_array());
    }

    #[test]
    fn secure_script_signature_payload_is_fixed() {
        assert_eq!(
            secure_script_signature_payload("abc", 100),
            "abc:100".to_owned()
        );
    }

    #[test]
    fn secure_script_status_filter_accepts_known_statuses() {
        assert!(validate_secure_script_status_filter(Some("draft")).is_ok());
        assert!(validate_secure_script_status_filter(Some("published")).is_ok());
        assert!(validate_secure_script_status_filter(Some("deprecated")).is_ok());
        assert!(validate_secure_script_status_filter(Some("unknown")).is_err());
    }

    #[test]
    fn create_secure_script_input_rejects_invalid_fields() {
        let invalid_version_code = CreateSecureScriptInput {
            name: "init".to_owned(),
            version: "1.0.0".to_owned(),
            version_code: 0,
            required_features: None,
            expires_at: None,
        };
        let invalid_required_features = CreateSecureScriptInput {
            name: "init".to_owned(),
            version: "1.0.0".to_owned(),
            version_code: 100,
            required_features: Some(serde_json::json!({ "feature": "script" })),
            expires_at: None,
        };

        assert!(validate_create_secure_script_input(&invalid_version_code).is_err());
        assert!(validate_create_secure_script_input(&invalid_required_features).is_err());
    }

    #[test]
    fn update_secure_script_content_input_rejects_invalid_version_fields() {
        let invalid_version = UpdateSecureScriptContentInput {
            content_base64: "YWJj".to_owned(),
            version: Some(" ".to_owned()),
            version_code: None,
        };
        let invalid_version_code = UpdateSecureScriptContentInput {
            content_base64: "YWJj".to_owned(),
            version: None,
            version_code: Some(0),
        };

        assert!(validate_update_secure_script_content_input(&invalid_version).is_err());
        assert!(validate_update_secure_script_content_input(&invalid_version_code).is_err());
    }

    #[test]
    fn required_features_must_be_present_in_license_features() {
        let license_features = serde_json::json!(["pro", "script"]);
        let required_features = serde_json::json!(["script"]);
        let missing_features = serde_json::json!(["enterprise"]);

        assert!(ensure_required_features(&license_features, &required_features).is_ok());
        assert!(ensure_required_features(&license_features, &missing_features).is_err());
    }
}
