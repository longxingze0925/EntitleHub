use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    crypto::token::hash_token,
    error::AppError,
    modules::license::{model::License, repository::LicenseRepository},
};

#[derive(Clone)]
pub struct LicenseService {
    repository: LicenseRepository,
    token_hash_pepper: String,
}

#[derive(Debug, Clone)]
pub struct ValidLicense {
    pub license: License,
}

impl LicenseService {
    pub fn new(pool: PgPool, token_hash_pepper: impl Into<String>) -> Self {
        Self {
            repository: LicenseRepository::new(pool),
            token_hash_pepper: token_hash_pepper.into(),
        }
    }

    pub async fn validate_license_key(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        license_key: &str,
        now: DateTime<Utc>,
    ) -> Result<ValidLicense, AppError> {
        let normalized_key = normalize_license_key(license_key)?;
        let license_key_hash = hash_token(&self.token_hash_pepper, &normalized_key)?;
        let license = self
            .repository
            .find_by_app_and_key_hash(tenant_id, app_id, &license_key_hash)
            .await?
            .ok_or_else(AppError::license_not_found)?;

        validate_license_record(&license, now)?;

        Ok(ValidLicense { license })
    }
}

pub fn validate_license_record(license: &License, now: DateTime<Utc>) -> Result<(), AppError> {
    if license.status != "active" {
        return Err(AppError::invalid_license_state(format!(
            "license is {}",
            license.status
        )));
    }

    if license.revoked_at.is_some() {
        return Err(AppError::license_invalid("license is revoked"));
    }

    if let Some(starts_at) = license.starts_at {
        if starts_at > now {
            return Err(AppError::license_invalid("license is not active yet"));
        }
    }

    if let Some(expires_at) = license.expires_at {
        if expires_at <= now {
            return Err(AppError::license_expired());
        }
    }

    Ok(())
}

fn normalize_license_key(license_key: &str) -> Result<String, AppError> {
    let license_key = license_key.trim();
    if license_key.is_empty() {
        return Err(AppError::license_invalid("license key is required"));
    }

    Ok(license_key.to_owned())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use crate::modules::license::model::License;

    use super::validate_license_record;

    #[test]
    fn active_unexpired_license_is_valid() {
        let now = Utc::now();
        let license = test_license(
            "active",
            Some(now - Duration::seconds(1)),
            Some(now + Duration::days(1)),
            None,
        );

        assert!(validate_license_record(&license, now).is_ok());
    }

    #[test]
    fn expired_license_is_rejected() {
        let now = Utc::now();
        let license = test_license("active", Some(now - Duration::days(2)), Some(now), None);

        assert!(validate_license_record(&license, now).is_err());
    }

    #[test]
    fn suspended_license_is_rejected() {
        let now = Utc::now();
        let license = test_license("suspended", Some(now - Duration::days(1)), None, None);

        assert!(validate_license_record(&license, now).is_err());
    }

    #[test]
    fn revoked_license_is_rejected() {
        let now = Utc::now();
        let license = test_license(
            "active",
            Some(now - Duration::days(1)),
            None,
            Some(now - Duration::seconds(1)),
        );

        assert!(validate_license_record(&license, now).is_err());
    }

    #[test]
    fn future_license_is_rejected() {
        let now = Utc::now();
        let license = test_license("active", Some(now + Duration::days(1)), None, None);

        assert!(validate_license_record(&license, now).is_err());
    }

    fn test_license(
        status: &str,
        starts_at: Option<chrono::DateTime<Utc>>,
        expires_at: Option<chrono::DateTime<Utc>>,
        revoked_at: Option<chrono::DateTime<Utc>>,
    ) -> License {
        License {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: None,
            license_key_hash: "hash".to_owned(),
            license_type: "standard".to_owned(),
            status: status.to_owned(),
            max_devices: 1,
            features: serde_json::json!([]),
            starts_at,
            expires_at,
            revoked_at,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }
}
