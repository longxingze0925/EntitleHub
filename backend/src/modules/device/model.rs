use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{crypto::signing::parse_ed25519_public_key, error::AppError};

#[derive(Debug, Clone, FromRow)]
pub struct Device {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub license_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os: Option<String>,
    pub app_version: Option<String>,
    pub status: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewDevice {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub license_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os: Option<String>,
    pub app_version: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct DeviceBindInput {
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub license_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os: Option<String>,
    pub app_version: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, FromRow)]
pub struct DeviceKey {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub device_id: Uuid,
    pub public_key: String,
    pub algorithm: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub rotated_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewDeviceKey {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub device_id: Uuid,
    pub public_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceListQuery {
    pub app_id: Option<Uuid>,
    pub customer_id: Option<Uuid>,
    pub license_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub status: Option<String>,
    pub machine_id: Option<String>,
    pub include_history: Option<bool>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct DeviceSummary {
    pub id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub license_id: Option<Uuid>,
    pub subscription_id: Option<Uuid>,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os: Option<String>,
    pub app_version: Option<String>,
    pub status: String,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Device> for DeviceSummary {
    fn from(device: Device) -> Self {
        Self {
            id: device.id,
            app_id: device.app_id,
            customer_id: device.customer_id,
            license_id: device.license_id,
            subscription_id: device.subscription_id,
            machine_id: device.machine_id,
            device_name: device.device_name,
            os: device.os,
            app_version: device.app_version,
            status: device.status,
            first_seen_at: device.first_seen_at,
            last_seen_at: device.last_seen_at,
            created_at: device.created_at,
            updated_at: device.updated_at,
        }
    }
}

impl NewDevice {
    pub fn from_bind_input(input: DeviceBindInput) -> Result<Self, AppError> {
        let machine_id = normalize_machine_id(&input.machine_id)?;

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id: input.tenant_id,
            app_id: input.app_id,
            customer_id: input.customer_id,
            license_id: input.license_id,
            subscription_id: input.subscription_id,
            machine_id,
            device_name: clean_optional(input.device_name),
            os: clean_optional(input.os),
            app_version: clean_optional(input.app_version),
            metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        })
    }
}

impl NewDeviceKey {
    pub fn new(
        tenant_id: Uuid,
        app_id: Uuid,
        device_id: Uuid,
        public_key: String,
    ) -> Result<Self, AppError> {
        let public_key = public_key.trim().to_owned();
        if public_key.is_empty() {
            return Err(AppError::validation_failed("device_public_key is required"));
        }
        parse_ed25519_public_key(&public_key)
            .map_err(|_| AppError::validation_failed("device_public_key invalid"))?;

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id,
            device_id,
            public_key,
        })
    }
}

pub fn normalize_machine_id(machine_id: &str) -> Result<String, AppError> {
    let machine_id = machine_id.trim().to_owned();
    if machine_id.is_empty() {
        return Err(AppError::validation_failed("machine_id is required"));
    }

    Ok(machine_id)
}

pub fn validate_device_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(status, "active" | "disabled" | "blacklisted" | "unbound") {
        return Ok(());
    }

    Err(AppError::validation_failed("device status is invalid"))
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

    use crate::crypto::signing::generate_ed25519_key;

    use super::{validate_device_status_filter, DeviceBindInput, NewDevice, NewDeviceKey};

    #[test]
    fn new_device_normalizes_machine_id_and_optional_fields() {
        let device = NewDevice::from_bind_input(DeviceBindInput {
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: None,
            license_id: None,
            subscription_id: None,
            machine_id: " machine ".to_owned(),
            device_name: Some(" PC ".to_owned()),
            os: Some(" ".to_owned()),
            app_version: None,
            metadata: None,
        })
        .expect("device should be valid");

        assert_eq!(device.machine_id, "machine");
        assert_eq!(device.device_name, Some("PC".to_owned()));
        assert_eq!(device.os, None);
    }

    #[test]
    fn new_device_rejects_blank_machine_id() {
        let result = NewDevice::from_bind_input(DeviceBindInput {
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: None,
            license_id: None,
            subscription_id: None,
            machine_id: " ".to_owned(),
            device_name: None,
            os: None,
            app_version: None,
            metadata: None,
        });

        assert!(result.is_err());
    }

    #[test]
    fn new_device_key_rejects_blank_public_key() {
        assert!(NewDeviceKey::new(Uuid::nil(), Uuid::nil(), Uuid::nil(), " ".to_owned()).is_err());
        assert!(NewDeviceKey::new(
            Uuid::nil(),
            Uuid::nil(),
            Uuid::nil(),
            "public-key".to_owned()
        )
        .is_err());
    }

    #[test]
    fn new_device_key_accepts_valid_ed25519_public_key() {
        let key = generate_ed25519_key().expect("key should generate");

        let device_key = NewDeviceKey::new(
            Uuid::nil(),
            Uuid::nil(),
            Uuid::nil(),
            key.public_key_pem.clone(),
        )
        .expect("device key should be valid");

        assert_eq!(device_key.public_key, key.public_key_pem.trim());
    }

    #[test]
    fn device_status_filter_accepts_known_statuses() {
        assert!(validate_device_status_filter(Some("active")).is_ok());
        assert!(validate_device_status_filter(Some("disabled")).is_ok());
        assert!(validate_device_status_filter(Some("blacklisted")).is_ok());
        assert!(validate_device_status_filter(Some("unbound")).is_ok());
        assert!(validate_device_status_filter(Some("unknown")).is_err());
    }
}
