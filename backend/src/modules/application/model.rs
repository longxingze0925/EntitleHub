use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct Application {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub slug: Option<String>,
    pub app_key: String,
    pub app_secret_hash: String,
    pub auth_mode: String,
    pub status: String,
    pub heartbeat_interval_seconds: i32,
    pub offline_tolerance_seconds: i32,
    pub max_devices_default: i32,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewApplication {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub slug: Option<String>,
    pub app_key: String,
    pub app_secret_hash: String,
    pub auth_mode: String,
    pub heartbeat_interval_seconds: i32,
    pub offline_tolerance_seconds: i32,
    pub max_devices_default: i32,
    pub metadata: Value,
}

#[derive(Debug, Clone, FromRow)]
pub struct SigningKey {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub app_id: Option<Uuid>,
    pub key_scope: String,
    pub kid: String,
    pub alg: String,
    pub public_key_pem: String,
    pub private_key_envelope: Option<Value>,
    pub status: String,
    pub not_before: DateTime<Utc>,
    pub not_after: Option<DateTime<Utc>>,
    pub rotated_from_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub activated_at: Option<DateTime<Utc>>,
    pub retired_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewSigningKey {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub app_id: Option<Uuid>,
    pub key_scope: String,
    pub kid: String,
    pub public_key_pem: String,
    pub private_key_envelope: Value,
    pub rotated_from_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CreateApplicationInput {
    pub name: String,
    pub slug: Option<String>,
    pub auth_mode: Option<String>,
    pub heartbeat_interval_seconds: Option<i32>,
    pub offline_tolerance_seconds: Option<i32>,
    pub max_devices_default: Option<i32>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApplicationListQuery {
    pub keyword: Option<String>,
    pub status: Option<String>,
    pub include_history: Option<bool>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplicationListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateApplicationInput {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub auth_mode: Option<String>,
    pub status: Option<String>,
    pub heartbeat_interval_seconds: Option<i32>,
    pub offline_tolerance_seconds: Option<i32>,
    pub max_devices_default: Option<i32>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct UpdateApplication {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub auth_mode: Option<String>,
    pub status: Option<String>,
    pub heartbeat_interval_seconds: Option<i32>,
    pub offline_tolerance_seconds: Option<i32>,
    pub max_devices_default: Option<i32>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ApplicationSummary {
    pub id: Uuid,
    pub name: String,
    pub slug: Option<String>,
    pub app_key: String,
    pub auth_mode: String,
    pub status: String,
    pub heartbeat_interval_seconds: i32,
    pub offline_tolerance_seconds: i32,
    pub max_devices_default: i32,
    pub metadata: Value,
}

impl NewApplication {
    pub fn from_input(
        tenant_id: Uuid,
        input: CreateApplicationInput,
        app_key: String,
        app_secret_hash: String,
    ) -> Result<Self, AppError> {
        let name = clean_required(input.name, "name")?;
        let slug = normalize_slug(input.slug)?;
        let auth_mode = normalize_auth_mode(input.auth_mode)?;
        let heartbeat_interval_seconds = input.heartbeat_interval_seconds.unwrap_or(3600);
        let offline_tolerance_seconds = input.offline_tolerance_seconds.unwrap_or(86_400);
        let max_devices_default = input.max_devices_default.unwrap_or(1);

        validate_timing(heartbeat_interval_seconds, offline_tolerance_seconds)?;
        validate_max_devices_default(max_devices_default)?;

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            name,
            slug,
            app_key,
            app_secret_hash,
            auth_mode,
            heartbeat_interval_seconds,
            offline_tolerance_seconds,
            max_devices_default,
            metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        })
    }
}

impl UpdateApplication {
    pub fn from_input(
        input: UpdateApplicationInput,
        current: &Application,
    ) -> Result<Self, AppError> {
        let name = input
            .name
            .map(|value| clean_required(value, "name"))
            .transpose()?;
        let slug = input
            .slug
            .map(Some)
            .map(normalize_slug)
            .transpose()?
            .flatten();
        let auth_mode = input
            .auth_mode
            .map(Some)
            .map(normalize_auth_mode)
            .transpose()?;
        let status = input.status.map(normalize_status).transpose()?;
        let heartbeat_interval_seconds = input.heartbeat_interval_seconds;
        let offline_tolerance_seconds = input.offline_tolerance_seconds;
        let next_heartbeat =
            heartbeat_interval_seconds.unwrap_or(current.heartbeat_interval_seconds);
        let next_offline_tolerance =
            offline_tolerance_seconds.unwrap_or(current.offline_tolerance_seconds);
        let max_devices_default = input.max_devices_default;

        validate_timing(next_heartbeat, next_offline_tolerance)?;
        if let Some(value) = max_devices_default {
            validate_max_devices_default(value)?;
        }

        Ok(Self {
            name,
            slug,
            auth_mode,
            status,
            heartbeat_interval_seconds,
            offline_tolerance_seconds,
            max_devices_default,
            metadata: input.metadata,
        })
    }
}

impl NewSigningKey {
    pub fn app_request(
        tenant_id: Uuid,
        app_id: Uuid,
        kid: String,
        public_key_pem: String,
        private_key_envelope: Value,
        created_by: Uuid,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: Some(tenant_id),
            app_id: Some(app_id),
            key_scope: "app_request".to_owned(),
            kid,
            public_key_pem,
            private_key_envelope,
            rotated_from_id: None,
            created_by: Some(created_by),
        }
    }

    pub fn with_rotated_from_id(mut self, rotated_from_id: Option<Uuid>) -> Self {
        self.rotated_from_id = rotated_from_id;
        self
    }

    pub fn with_created_by(mut self, created_by: Option<Uuid>) -> Self {
        self.created_by = created_by;
        self
    }

    pub fn release_file(
        tenant_id: Uuid,
        app_id: Uuid,
        kid: String,
        public_key_pem: String,
        private_key_envelope: Value,
        created_by: Uuid,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: Some(tenant_id),
            app_id: Some(app_id),
            key_scope: "release_file".to_owned(),
            kid,
            public_key_pem,
            private_key_envelope,
            rotated_from_id: None,
            created_by: Some(created_by),
        }
    }

    pub fn secure_script(
        tenant_id: Uuid,
        app_id: Uuid,
        kid: String,
        public_key_pem: String,
        private_key_envelope: Value,
        created_by: Uuid,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: Some(tenant_id),
            app_id: Some(app_id),
            key_scope: "secure_script".to_owned(),
            kid,
            public_key_pem,
            private_key_envelope,
            rotated_from_id: None,
            created_by: Some(created_by),
        }
    }

    pub fn jwt_access_token(
        kid: String,
        public_key_pem: String,
        private_key_envelope: Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: None,
            app_id: None,
            key_scope: "jwt_access_token".to_owned(),
            kid,
            public_key_pem,
            private_key_envelope,
            rotated_from_id: None,
            created_by: None,
        }
    }
}

impl From<Application> for ApplicationSummary {
    fn from(application: Application) -> Self {
        Self {
            id: application.id,
            name: application.name,
            slug: application.slug,
            app_key: application.app_key,
            auth_mode: application.auth_mode,
            status: application.status,
            heartbeat_interval_seconds: application.heartbeat_interval_seconds,
            offline_tolerance_seconds: application.offline_tolerance_seconds,
            max_devices_default: application.max_devices_default,
            metadata: application.metadata,
        }
    }
}

fn clean_required(value: String, field: &'static str) -> Result<String, AppError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::validation_failed(format!("{field} is required")));
    }

    Ok(value)
}

fn normalize_slug(slug: Option<String>) -> Result<Option<String>, AppError> {
    let Some(slug) = slug else {
        return Ok(None);
    };

    let slug = slug.trim().to_lowercase();
    if slug.is_empty() {
        return Ok(None);
    }

    let valid = slug
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    if !valid {
        return Err(AppError::validation_failed(
            "slug may only contain lowercase letters, numbers, and hyphen",
        ));
    }

    Ok(Some(slug))
}

fn normalize_auth_mode(auth_mode: Option<String>) -> Result<String, AppError> {
    let auth_mode = auth_mode
        .map(|value| value.trim().to_lowercase())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "both".to_owned());

    if matches!(auth_mode.as_str(), "license" | "subscription" | "both") {
        return Ok(auth_mode);
    }

    Err(AppError::validation_failed("auth_mode is invalid"))
}

pub fn validate_application_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(status, "active" | "disabled" | "archived") {
        return Ok(());
    }

    Err(AppError::validation_failed("application status is invalid"))
}

fn normalize_status(status: String) -> Result<String, AppError> {
    let status = status.trim().to_lowercase();
    if matches!(status.as_str(), "active" | "disabled" | "archived") {
        return Ok(status);
    }

    Err(AppError::validation_failed("application status is invalid"))
}

fn validate_timing(heartbeat: i32, offline_tolerance: i32) -> Result<(), AppError> {
    if heartbeat <= 0 {
        return Err(AppError::validation_failed(
            "heartbeat_interval_seconds must be greater than 0",
        ));
    }

    if offline_tolerance < heartbeat {
        return Err(AppError::validation_failed(
            "offline_tolerance_seconds must be greater than or equal to heartbeat_interval_seconds",
        ));
    }

    Ok(())
}

fn validate_max_devices_default(value: i32) -> Result<(), AppError> {
    if value < 0 {
        return Err(AppError::validation_failed(
            "max_devices_default must be greater than or equal to 0",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{
        validate_application_status_filter, CreateApplicationInput, NewApplication,
        UpdateApplication, UpdateApplicationInput,
    };

    #[test]
    fn new_application_applies_defaults() {
        let application = NewApplication::from_input(
            Uuid::nil(),
            CreateApplicationInput {
                name: " My App ".to_owned(),
                slug: None,
                auth_mode: None,
                heartbeat_interval_seconds: None,
                offline_tolerance_seconds: None,
                max_devices_default: None,
                metadata: None,
            },
            "app_key".to_owned(),
            "secret_hash".to_owned(),
        )
        .expect("application should be valid");

        assert_eq!(application.name, "My App");
        assert_eq!(application.auth_mode, "both");
        assert_eq!(application.heartbeat_interval_seconds, 3600);
        assert_eq!(application.offline_tolerance_seconds, 86_400);
        assert_eq!(application.max_devices_default, 1);
    }

    #[test]
    fn new_application_rejects_invalid_auth_mode() {
        let result = NewApplication::from_input(
            Uuid::nil(),
            CreateApplicationInput {
                name: "My App".to_owned(),
                slug: None,
                auth_mode: Some("invalid".to_owned()),
                heartbeat_interval_seconds: None,
                offline_tolerance_seconds: None,
                max_devices_default: None,
                metadata: None,
            },
            "app_key".to_owned(),
            "secret_hash".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn new_application_rejects_tolerance_below_heartbeat() {
        let result = NewApplication::from_input(
            Uuid::nil(),
            CreateApplicationInput {
                name: "My App".to_owned(),
                slug: None,
                auth_mode: None,
                heartbeat_interval_seconds: Some(60),
                offline_tolerance_seconds: Some(30),
                max_devices_default: None,
                metadata: None,
            },
            "app_key".to_owned(),
            "secret_hash".to_owned(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn update_application_validates_timing_against_current_values() {
        let application = NewApplication::from_input(
            Uuid::nil(),
            CreateApplicationInput {
                name: "My App".to_owned(),
                slug: None,
                auth_mode: None,
                heartbeat_interval_seconds: Some(60),
                offline_tolerance_seconds: Some(120),
                max_devices_default: None,
                metadata: None,
            },
            "app_key".to_owned(),
            "secret_hash".to_owned(),
        )
        .expect("application should be valid");
        let current = crate::modules::application::model::Application {
            id: application.id,
            tenant_id: application.tenant_id,
            name: application.name,
            slug: application.slug,
            app_key: application.app_key,
            app_secret_hash: application.app_secret_hash,
            auth_mode: application.auth_mode,
            status: "active".to_owned(),
            heartbeat_interval_seconds: application.heartbeat_interval_seconds,
            offline_tolerance_seconds: application.offline_tolerance_seconds,
            max_devices_default: application.max_devices_default,
            metadata: application.metadata,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            deleted_at: None,
        };

        let result = UpdateApplication::from_input(
            UpdateApplicationInput {
                name: None,
                slug: None,
                auth_mode: None,
                status: None,
                heartbeat_interval_seconds: Some(180),
                offline_tolerance_seconds: None,
                max_devices_default: None,
                metadata: None,
            },
            &current,
        );

        assert!(result.is_err());
    }

    #[test]
    fn status_filter_accepts_known_application_statuses() {
        assert!(validate_application_status_filter(Some("active")).is_ok());
        assert!(validate_application_status_filter(Some("disabled")).is_ok());
        assert!(validate_application_status_filter(Some("archived")).is_ok());
        assert!(validate_application_status_filter(Some("deleted")).is_err());
    }

    #[test]
    fn jwt_access_token_key_is_global_and_can_track_rotation_actor() {
        let rotated_from_id = Uuid::new_v4();
        let created_by = Uuid::new_v4();

        let signing_key = super::NewSigningKey::jwt_access_token(
            "kid".to_owned(),
            "public-key".to_owned(),
            serde_json::json!({ "encrypted": true }),
        )
        .with_rotated_from_id(Some(rotated_from_id))
        .with_created_by(Some(created_by));

        assert_eq!(signing_key.tenant_id, None);
        assert_eq!(signing_key.app_id, None);
        assert_eq!(signing_key.key_scope, "jwt_access_token");
        assert_eq!(signing_key.rotated_from_id, Some(rotated_from_id));
        assert_eq!(signing_key.created_by, Some(created_by));
    }
}
