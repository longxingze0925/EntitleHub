use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct License {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub license_key_hash: String,
    pub license_type: String,
    pub status: String,
    pub max_devices: i32,
    pub features: Value,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewLicense {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub license_key_hash: String,
    pub license_type: String,
    pub max_devices: i32,
    pub features: Value,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct CreateLicenseInput {
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub license_type: Option<String>,
    pub max_devices: Option<i32>,
    pub features: Option<Value>,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseListQuery {
    pub app_id: Option<Uuid>,
    pub customer_id: Option<Uuid>,
    pub status: Option<String>,
    pub keyword: Option<String>,
    pub include_history: Option<bool>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LicenseListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Deserialize)]
pub struct RenewLicenseInput {
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ResetLicenseDevicesInput {
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LicenseSummary {
    pub id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub license_type: String,
    pub status: String,
    pub max_devices: i32,
    pub features: Value,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl NewLicense {
    pub fn from_input(
        tenant_id: Uuid,
        input: CreateLicenseInput,
        license_key_hash: String,
    ) -> Result<Self, AppError> {
        let license_type = normalize_license_type(input.license_type)?;
        let max_devices = input.max_devices.unwrap_or(1);
        validate_max_devices(max_devices)?;
        let features = normalize_features(input.features)?;
        let starts_at = input.starts_at.or_else(|| Some(Utc::now()));
        validate_license_window(starts_at, input.expires_at)?;

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id: input.app_id,
            customer_id: input.customer_id,
            license_key_hash,
            license_type,
            max_devices,
            features,
            starts_at,
            expires_at: input.expires_at,
            metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        })
    }
}

impl From<License> for LicenseSummary {
    fn from(license: License) -> Self {
        Self {
            id: license.id,
            app_id: license.app_id,
            customer_id: license.customer_id,
            license_type: license.license_type,
            status: license.status,
            max_devices: license.max_devices,
            features: license.features,
            starts_at: license.starts_at,
            expires_at: license.expires_at,
            revoked_at: license.revoked_at,
            metadata: license.metadata,
        }
    }
}

pub fn validate_license_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(status, "active" | "suspended" | "revoked" | "expired") {
        return Ok(());
    }

    Err(AppError::validation_failed("license status is invalid"))
}

pub fn validate_renew_expires_at(
    current: &License,
    expires_at: DateTime<Utc>,
) -> Result<(), AppError> {
    if let Some(starts_at) = current.starts_at {
        if expires_at <= starts_at {
            return Err(AppError::validation_failed(
                "expires_at must be greater than starts_at",
            ));
        }
    }

    Ok(())
}

pub fn normalize_reset_device_reason(reason: Option<String>) -> Result<String, AppError> {
    let reason = reason
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::validation_failed("reason is required"))?;

    if reason.chars().count() > 500 {
        return Err(AppError::validation_failed("reason is too long"));
    }

    Ok(reason)
}

fn normalize_license_type(license_type: Option<String>) -> Result<String, AppError> {
    let license_type = license_type
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "standard".to_owned());

    if matches!(license_type.as_str(), "standard" | "trial" | "enterprise") {
        return Ok(license_type);
    }

    Err(AppError::validation_failed("license type is invalid"))
}

fn normalize_features(features: Option<Value>) -> Result<Value, AppError> {
    let features = features.unwrap_or_else(|| serde_json::json!([]));
    if features.is_array() {
        return Ok(features);
    }

    Err(AppError::validation_failed("features must be an array"))
}

fn validate_license_window(
    starts_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), AppError> {
    if let (Some(starts_at), Some(expires_at)) = (starts_at, expires_at) {
        if expires_at <= starts_at {
            return Err(AppError::validation_failed(
                "expires_at must be greater than starts_at",
            ));
        }
    }

    Ok(())
}

fn validate_max_devices(max_devices: i32) -> Result<(), AppError> {
    if max_devices < 0 {
        return Err(AppError::validation_failed(
            "max_devices must be greater than or equal to 0",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use super::{
        normalize_reset_device_reason, validate_license_status_filter, CreateLicenseInput,
        NewLicense,
    };

    #[test]
    fn new_license_applies_defaults() {
        let license = NewLicense::from_input(
            Uuid::nil(),
            CreateLicenseInput {
                app_id: Uuid::nil(),
                customer_id: None,
                license_type: None,
                max_devices: None,
                features: None,
                starts_at: None,
                expires_at: None,
                metadata: None,
            },
            "hash".to_owned(),
        )
        .expect("license should be valid");

        assert_eq!(license.license_type, "standard");
        assert_eq!(license.max_devices, 1);
        assert!(license.features.is_array());
        assert!(license.starts_at.is_some());
    }

    #[test]
    fn new_license_rejects_feature_object() {
        let result = NewLicense::from_input(
            Uuid::nil(),
            CreateLicenseInput {
                app_id: Uuid::nil(),
                customer_id: None,
                license_type: None,
                max_devices: None,
                features: Some(serde_json::json!({ "pro": true })),
                starts_at: None,
                expires_at: None,
                metadata: None,
            },
            "hash".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn new_license_rejects_invalid_window() {
        let now = Utc::now();
        let result = NewLicense::from_input(
            Uuid::nil(),
            CreateLicenseInput {
                app_id: Uuid::nil(),
                customer_id: None,
                license_type: None,
                max_devices: None,
                features: None,
                starts_at: Some(now),
                expires_at: Some(now - Duration::seconds(1)),
                metadata: None,
            },
            "hash".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn new_license_rejects_past_expiry_without_explicit_start() {
        let result = NewLicense::from_input(
            Uuid::nil(),
            CreateLicenseInput {
                app_id: Uuid::nil(),
                customer_id: None,
                license_type: None,
                max_devices: None,
                features: None,
                starts_at: None,
                expires_at: Some(Utc::now() - Duration::seconds(1)),
                metadata: None,
            },
            "hash".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn status_filter_accepts_known_license_statuses() {
        assert!(validate_license_status_filter(Some("active")).is_ok());
        assert!(validate_license_status_filter(Some("revoked")).is_ok());
        assert!(validate_license_status_filter(Some("unknown")).is_err());
    }

    #[test]
    fn reset_device_reason_is_required_and_trimmed() {
        assert_eq!(
            normalize_reset_device_reason(Some(" customer replaced laptop ".to_owned()))
                .expect("reason should normalize"),
            "customer replaced laptop"
        );
        assert!(normalize_reset_device_reason(None).is_err());
        assert!(normalize_reset_device_reason(Some("   ".to_owned())).is_err());
    }

    #[test]
    fn reset_device_reason_rejects_long_values() {
        let reason = "a".repeat(501);

        assert!(normalize_reset_device_reason(Some(reason)).is_err());
    }
}
