use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct Subscription {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Uuid,
    pub plan: String,
    pub status: String,
    pub max_devices: i32,
    pub features: Value,
    pub starts_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewSubscription {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Uuid,
    pub plan: String,
    pub max_devices: i32,
    pub features: Value,
    pub starts_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionInput {
    pub app_id: Uuid,
    pub customer_id: Uuid,
    pub plan: String,
    pub max_devices: Option<i32>,
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub features: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SubscriptionListQuery {
    pub app_id: Option<Uuid>,
    pub customer_id: Option<Uuid>,
    pub status: Option<String>,
    pub keyword: Option<String>,
    pub include_history: Option<bool>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionListMeta {
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionSummary {
    pub id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Uuid,
    pub plan: String,
    pub status: String,
    pub max_devices: i32,
    pub features: Value,
    pub starts_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub metadata: Value,
}

impl NewSubscription {
    pub fn from_input(tenant_id: Uuid, input: CreateSubscriptionInput) -> Result<Self, AppError> {
        let plan = normalize_plan(input.plan)?;
        let max_devices = input.max_devices.unwrap_or(1);
        validate_max_devices(max_devices)?;
        let features = normalize_features(input.features)?;
        let starts_at = input.starts_at.unwrap_or_else(Utc::now);
        validate_subscription_window(starts_at, input.expires_at)?;

        Ok(Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id: input.app_id,
            customer_id: input.customer_id,
            plan,
            max_devices,
            features,
            starts_at,
            expires_at: input.expires_at,
            metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        })
    }
}

impl From<Subscription> for SubscriptionSummary {
    fn from(subscription: Subscription) -> Self {
        Self {
            id: subscription.id,
            app_id: subscription.app_id,
            customer_id: subscription.customer_id,
            plan: subscription.plan,
            status: subscription.status,
            max_devices: subscription.max_devices,
            features: subscription.features,
            starts_at: subscription.starts_at,
            expires_at: subscription.expires_at,
            cancelled_at: subscription.cancelled_at,
            metadata: subscription.metadata,
        }
    }
}

pub fn validate_subscription_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(
        status,
        "active" | "trialing" | "past_due" | "cancelled" | "expired"
    ) {
        return Ok(());
    }

    Err(AppError::validation_failed(
        "subscription status is invalid",
    ))
}

fn normalize_plan(plan: String) -> Result<String, AppError> {
    let plan = plan.trim().to_lowercase();
    if plan.is_empty() {
        return Err(AppError::validation_failed("plan is required"));
    }

    Ok(plan)
}

fn normalize_features(features: Option<Value>) -> Result<Value, AppError> {
    let features = features.unwrap_or_else(|| serde_json::json!([]));
    if features.is_array() {
        return Ok(features);
    }

    Err(AppError::validation_failed("features must be an array"))
}

fn validate_subscription_window(
    starts_at: DateTime<Utc>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), AppError> {
    if let Some(expires_at) = expires_at {
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

    use super::{validate_subscription_status_filter, CreateSubscriptionInput, NewSubscription};

    #[test]
    fn new_subscription_applies_defaults() {
        let subscription = NewSubscription::from_input(
            Uuid::nil(),
            CreateSubscriptionInput {
                app_id: Uuid::nil(),
                customer_id: Uuid::nil(),
                plan: " Pro ".to_owned(),
                max_devices: None,
                starts_at: None,
                expires_at: None,
                features: None,
                metadata: None,
            },
        )
        .expect("subscription should be valid");

        assert_eq!(subscription.plan, "pro");
        assert_eq!(subscription.max_devices, 1);
        assert!(subscription.features.is_array());
    }

    #[test]
    fn new_subscription_rejects_feature_object() {
        let result = NewSubscription::from_input(
            Uuid::nil(),
            CreateSubscriptionInput {
                app_id: Uuid::nil(),
                customer_id: Uuid::nil(),
                plan: "pro".to_owned(),
                max_devices: None,
                starts_at: None,
                expires_at: None,
                features: Some(serde_json::json!({ "pro": true })),
                metadata: None,
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn new_subscription_rejects_invalid_window() {
        let now = Utc::now();
        let result = NewSubscription::from_input(
            Uuid::nil(),
            CreateSubscriptionInput {
                app_id: Uuid::nil(),
                customer_id: Uuid::nil(),
                plan: "pro".to_owned(),
                max_devices: None,
                starts_at: Some(now),
                expires_at: Some(now - Duration::seconds(1)),
                features: None,
                metadata: None,
            },
        );

        assert!(result.is_err());
    }

    #[test]
    fn status_filter_accepts_known_subscription_statuses() {
        assert!(validate_subscription_status_filter(Some("active")).is_ok());
        assert!(validate_subscription_status_filter(Some("trialing")).is_ok());
        assert!(validate_subscription_status_filter(Some("cancelled")).is_ok());
        assert!(validate_subscription_status_filter(Some("unknown")).is_err());
    }
}
