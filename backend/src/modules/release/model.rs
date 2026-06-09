use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;

const MAX_FILE_NAME_LEN: usize = 255;
const MAX_VERSION_LEN: usize = 128;

#[derive(Debug, Clone, FromRow)]
pub struct ReleaseFile {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub storage_key: String,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
    pub signing_key_id: Uuid,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewReleaseFile {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub storage_key: String,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
    pub signing_key_id: Uuid,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub metadata: Value,
}

#[derive(Debug, Clone, FromRow)]
pub struct Release {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub file_id: Uuid,
    pub version: String,
    pub version_code: i64,
    pub status: String,
    pub changelog: Option<String>,
    pub force_update: bool,
    pub signing_key_id: Option<Uuid>,
    pub signature_kid: Option<String>,
    pub signature: Option<String>,
    pub signature_alg: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub deprecated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewRelease {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub file_id: Uuid,
    pub version: String,
    pub version_code: i64,
    pub changelog: Option<String>,
    pub force_update: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct ReleaseWithFile {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub file_id: Uuid,
    pub version: String,
    pub version_code: i64,
    pub status: String,
    pub changelog: Option<String>,
    pub force_update: bool,
    pub signing_key_id: Uuid,
    pub release_signature_kid: String,
    pub release_signature: String,
    pub release_signature_alg: String,
    pub published_at: Option<DateTime<Utc>>,
    pub deprecated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct DownloadToken {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub device_id: Uuid,
    pub file_id: Uuid,
    pub token_hash: String,
    pub kind: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewDownloadToken {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub device_id: Uuid,
    pub file_id: Uuid,
    pub token_hash: String,
    pub kind: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ValidatedDownload {
    pub file_id: Uuid,
    pub device_id: Uuid,
    pub storage_key: String,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterReleaseFileInput {
    pub storage_key: Option<String>,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CreateReleaseInput {
    pub file_id: Uuid,
    pub version: String,
    pub version_code: i64,
    pub changelog: Option<String>,
    pub force_update: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateReleaseInput {
    pub version: String,
    pub version_code: i64,
    pub changelog: Option<String>,
    pub force_update: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct UpdateRelease {
    pub version: String,
    pub version_code: i64,
    pub changelog: Option<String>,
    pub force_update: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseListQuery {
    pub status: Option<String>,
    pub include_history: Option<bool>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct ReleaseFileSummary {
    pub id: Uuid,
    pub storage_key: String,
    pub file_name: String,
    pub file_size: i64,
    pub sha256: String,
    pub signature_kid: String,
    pub signature: String,
    pub signature_alg: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ReleaseSummary {
    pub id: Uuid,
    pub app_id: Uuid,
    pub file_id: Uuid,
    pub version: String,
    pub version_code: i64,
    pub status: String,
    pub changelog: Option<String>,
    pub force_update: bool,
    pub signature_kid: Option<String>,
    pub signature: Option<String>,
    pub signature_alg: Option<String>,
    pub published_at: Option<DateTime<Utc>>,
    pub deprecated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl NewReleaseFile {
    pub fn from_input(
        tenant_id: Uuid,
        app_id: Uuid,
        input: RegisterReleaseFileInput,
        signing_key_id: Uuid,
        signature_kid: String,
        signature: String,
    ) -> Result<Self, AppError> {
        let id = Uuid::new_v4();
        let file_name = normalize_file_name(input.file_name)?;
        let file_size = validate_file_size(input.file_size)?;
        let sha256 = normalize_sha256(input.sha256)?;
        let storage_key = normalize_storage_key(input.storage_key, tenant_id, app_id, id)?;

        Ok(Self {
            id,
            tenant_id,
            app_id,
            storage_key,
            file_name,
            file_size,
            sha256,
            signing_key_id,
            signature_kid,
            signature,
            signature_alg: "Ed25519".to_owned(),
            metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        })
    }
}

impl NewRelease {
    pub fn from_input(
        tenant_id: Uuid,
        app_id: Uuid,
        input: CreateReleaseInput,
    ) -> Result<Self, AppError> {
        let version = normalize_version(input.version)?;
        let version_code = validate_version_code(input.version_code)?;
        let changelog = clean_optional(input.changelog);

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id,
            file_id: input.file_id,
            version,
            version_code,
            changelog,
            force_update: input.force_update.unwrap_or(false),
        })
    }
}

impl UpdateRelease {
    pub fn from_input(input: UpdateReleaseInput) -> Result<Self, AppError> {
        let version = normalize_version(input.version)?;
        let version_code = validate_version_code(input.version_code)?;
        let changelog = clean_optional(input.changelog);

        Ok(Self {
            version,
            version_code,
            changelog,
            force_update: input.force_update.unwrap_or(false),
        })
    }
}

impl NewDownloadToken {
    pub fn release_file(
        tenant_id: Uuid,
        app_id: Uuid,
        device_id: Uuid,
        file_id: Uuid,
        token_hash: String,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, AppError> {
        if expires_at <= Utc::now() {
            return Err(AppError::validation_failed(
                "download token expires_at must be in the future",
            ));
        }

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id,
            device_id,
            file_id,
            token_hash,
            kind: "release_file".to_owned(),
            expires_at,
        })
    }
}

impl From<ReleaseFile> for ReleaseFileSummary {
    fn from(file: ReleaseFile) -> Self {
        Self {
            id: file.id,
            storage_key: file.storage_key,
            file_name: file.file_name,
            file_size: file.file_size,
            sha256: file.sha256,
            signature_kid: file.signature_kid,
            signature: file.signature,
            signature_alg: file.signature_alg,
            metadata: file.metadata,
            created_at: file.created_at,
        }
    }
}

impl From<Release> for ReleaseSummary {
    fn from(release: Release) -> Self {
        Self {
            id: release.id,
            app_id: release.app_id,
            file_id: release.file_id,
            version: release.version,
            version_code: release.version_code,
            status: release.status,
            changelog: release.changelog,
            force_update: release.force_update,
            signature_kid: release.signature_kid,
            signature: release.signature,
            signature_alg: release.signature_alg,
            published_at: release.published_at,
            deprecated_at: release.deprecated_at,
            created_at: release.created_at,
            updated_at: release.updated_at,
        }
    }
}

pub fn release_file_signature_payload(sha256: &str, file_size: i64) -> String {
    format!("{sha256}:{file_size}")
}

pub fn release_metadata_signature_payload(
    app_id: Uuid,
    version: &str,
    version_code: i64,
    sha256: &str,
    file_size: i64,
    published_at_unix: i64,
) -> String {
    format!("{app_id}\n{version}\n{version_code}\n{sha256}\n{file_size}\n{published_at_unix}")
}

pub fn validate_download_file_name(file_name: &str) -> Result<String, AppError> {
    normalize_file_name(file_name.to_owned())
}

pub fn validate_register_release_file_input(
    input: &RegisterReleaseFileInput,
    tenant_id: Uuid,
    app_id: Uuid,
) -> Result<(), AppError> {
    normalize_file_name(input.file_name.clone())?;
    validate_file_size(input.file_size)?;
    normalize_sha256(input.sha256.clone())?;
    normalize_storage_key(input.storage_key.clone(), tenant_id, app_id, Uuid::nil())?;

    Ok(())
}

pub fn validate_release_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(status, "draft" | "published" | "deprecated" | "revoked") {
        return Ok(());
    }

    Err(AppError::validation_failed("release status is invalid"))
}

fn normalize_file_name(file_name: String) -> Result<String, AppError> {
    let file_name = file_name.trim().to_owned();
    if file_name.is_empty() {
        return Err(AppError::validation_failed("file_name is required"));
    }
    if file_name.len() > MAX_FILE_NAME_LEN {
        return Err(AppError::validation_failed("file_name is too long"));
    }
    if file_name.contains('/')
        || file_name.contains('\\')
        || file_name.contains('\0')
        || file_name == "."
        || file_name == ".."
        || file_name.starts_with('.')
        || !file_name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(AppError::validation_failed("file_name is invalid"));
    }

    Ok(file_name)
}

fn validate_file_size(file_size: i64) -> Result<i64, AppError> {
    if file_size <= 0 {
        return Err(AppError::validation_failed(
            "file_size must be greater than 0",
        ));
    }

    Ok(file_size)
}

fn normalize_sha256(sha256: String) -> Result<String, AppError> {
    let sha256 = sha256.trim().to_lowercase();
    if sha256.len() == 64 && sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Ok(sha256);
    }

    Err(AppError::validation_failed(
        "sha256 must be a 64-character hex string",
    ))
}

fn normalize_storage_key(
    storage_key: Option<String>,
    tenant_id: Uuid,
    app_id: Uuid,
    file_id: Uuid,
) -> Result<String, AppError> {
    let Some(storage_key) = storage_key else {
        return Ok(format!(
            "tenants/{tenant_id}/apps/{app_id}/releases/{file_id}"
        ));
    };

    let storage_key = storage_key.trim().to_owned();
    if storage_key.is_empty() {
        return Err(AppError::validation_failed("storage_key is required"));
    }
    if storage_key.starts_with('/')
        || storage_key.contains('\\')
        || storage_key.contains('\0')
        || storage_key.split('/').any(|part| part == "..")
    {
        return Err(AppError::validation_failed("storage_key is invalid"));
    }

    Ok(storage_key)
}

fn normalize_version(version: String) -> Result<String, AppError> {
    let version = version.trim().to_owned();
    if version.is_empty() {
        return Err(AppError::validation_failed("version is required"));
    }
    if version.len() > MAX_VERSION_LEN {
        return Err(AppError::validation_failed("version is too long"));
    }

    Ok(version)
}

fn validate_version_code(version_code: i64) -> Result<i64, AppError> {
    if version_code <= 0 {
        return Err(AppError::validation_failed(
            "version_code must be greater than 0",
        ));
    }

    Ok(version_code)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{
        release_file_signature_payload, release_metadata_signature_payload,
        validate_register_release_file_input, validate_release_status_filter, CreateReleaseInput,
        NewRelease, NewReleaseFile, RegisterReleaseFileInput, UpdateRelease, UpdateReleaseInput,
    };

    #[test]
    fn new_release_file_defaults_storage_key_and_normalizes_hash() {
        let file = NewReleaseFile::from_input(
            Uuid::nil(),
            Uuid::nil(),
            RegisterReleaseFileInput {
                storage_key: None,
                file_name: " app.zip ".to_owned(),
                file_size: 12,
                sha256: "A".repeat(64),
                metadata: None,
            },
            Uuid::nil(),
            "kid".to_owned(),
            "sig".to_owned(),
        )
        .expect("release file should be valid");

        assert_eq!(file.file_name, "app.zip");
        assert_eq!(file.sha256, "a".repeat(64));
        assert!(file.storage_key.contains("/releases/"));
    }

    #[test]
    fn new_release_file_rejects_path_traversal_file_name() {
        let result = NewReleaseFile::from_input(
            Uuid::nil(),
            Uuid::nil(),
            RegisterReleaseFileInput {
                storage_key: None,
                file_name: "../app.zip".to_owned(),
                file_size: 12,
                sha256: "a".repeat(64),
                metadata: None,
            },
            Uuid::nil(),
            "kid".to_owned(),
            "sig".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn register_release_file_input_rejects_invalid_fields() {
        let invalid_file_name = RegisterReleaseFileInput {
            storage_key: None,
            file_name: "../app.zip".to_owned(),
            file_size: 12,
            sha256: "a".repeat(64),
            metadata: None,
        };
        let invalid_storage_key = RegisterReleaseFileInput {
            storage_key: Some("/absolute/app.zip".to_owned()),
            file_name: "app.zip".to_owned(),
            file_size: 12,
            sha256: "a".repeat(64),
            metadata: None,
        };

        assert!(
            validate_register_release_file_input(&invalid_file_name, Uuid::nil(), Uuid::nil())
                .is_err()
        );
        assert!(validate_register_release_file_input(
            &invalid_storage_key,
            Uuid::nil(),
            Uuid::nil()
        )
        .is_err());
    }

    #[test]
    fn new_release_validates_version_code() {
        let result = NewRelease::from_input(
            Uuid::nil(),
            Uuid::nil(),
            CreateReleaseInput {
                file_id: Uuid::nil(),
                version: "1.0.0".to_owned(),
                version_code: 0,
                changelog: None,
                force_update: None,
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn update_release_normalizes_fields() {
        let input = UpdateReleaseInput {
            version: " 1.0.1 ".to_owned(),
            version_code: 101,
            changelog: Some(" fixed ".to_owned()),
            force_update: Some(true),
        };
        let update = UpdateRelease::from_input(input).expect("release update should be valid");

        assert_eq!(update.version, "1.0.1");
        assert_eq!(update.version_code, 101);
        assert_eq!(update.changelog.as_deref(), Some("fixed"));
        assert!(update.force_update);
    }

    #[test]
    fn download_file_name_rejects_path_traversal() {
        assert!(super::validate_download_file_name("../app.zip").is_err());
        assert!(super::validate_download_file_name("app.zip").is_ok());
    }

    #[test]
    fn signature_payload_is_fixed_for_sdk_verification() {
        assert_eq!(
            release_file_signature_payload("abc", 123),
            "abc:123".to_owned()
        );
        assert_eq!(
            release_metadata_signature_payload(Uuid::nil(), "1.0.0", 100, "abc", 123, 456),
            "00000000-0000-0000-0000-000000000000\n1.0.0\n100\nabc\n123\n456".to_owned()
        );
    }

    #[test]
    fn release_status_filter_accepts_known_statuses() {
        assert!(validate_release_status_filter(Some("draft")).is_ok());
        assert!(validate_release_status_filter(Some("published")).is_ok());
        assert!(validate_release_status_filter(Some("unknown")).is_err());
    }
}
