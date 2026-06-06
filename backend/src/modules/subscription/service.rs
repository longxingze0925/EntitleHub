use chrono::{DateTime, Utc};

use crate::{error::AppError, modules::subscription::model::Subscription};

pub fn validate_subscription_record(
    subscription: &Subscription,
    now: DateTime<Utc>,
) -> Result<(), AppError> {
    if !matches!(subscription.status.as_str(), "active" | "trialing") {
        return Err(AppError::subscription_inactive(format!(
            "subscription is {}",
            subscription.status
        )));
    }

    if subscription.cancelled_at.is_some() {
        return Err(AppError::subscription_inactive("subscription is cancelled"));
    }

    if subscription.starts_at > now {
        return Err(AppError::subscription_inactive(
            "subscription is not active yet",
        ));
    }

    if let Some(expires_at) = subscription.expires_at {
        if expires_at <= now {
            return Err(AppError::subscription_inactive("subscription is expired"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use uuid::Uuid;

    use crate::{error::AppError, modules::subscription::model::Subscription};

    use super::validate_subscription_record;

    #[test]
    fn active_unexpired_subscription_is_valid() {
        let now = Utc::now();
        let subscription = test_subscription("active", now - Duration::seconds(1), None, None);

        assert!(validate_subscription_record(&subscription, now).is_ok());
    }

    #[test]
    fn trialing_subscription_is_valid() {
        let now = Utc::now();
        let subscription = test_subscription("trialing", now - Duration::seconds(1), None, None);

        assert!(validate_subscription_record(&subscription, now).is_ok());
    }

    #[test]
    fn cancelled_subscription_is_rejected() {
        let now = Utc::now();
        let subscription = test_subscription("cancelled", now - Duration::days(1), None, Some(now));

        assert!(matches!(
            validate_subscription_record(&subscription, now),
            Err(AppError::SubscriptionInactive(_))
        ));
    }

    #[test]
    fn future_subscription_is_rejected() {
        let now = Utc::now();
        let subscription = test_subscription("active", now + Duration::days(1), None, None);

        assert!(matches!(
            validate_subscription_record(&subscription, now),
            Err(AppError::SubscriptionInactive(_))
        ));
    }

    #[test]
    fn expired_subscription_is_rejected() {
        let now = Utc::now();
        let subscription = test_subscription(
            "active",
            now - Duration::days(2),
            Some(now - Duration::seconds(1)),
            None,
        );

        assert!(matches!(
            validate_subscription_record(&subscription, now),
            Err(AppError::SubscriptionInactive(_))
        ));
    }

    fn test_subscription(
        status: &str,
        starts_at: chrono::DateTime<Utc>,
        expires_at: Option<chrono::DateTime<Utc>>,
        cancelled_at: Option<chrono::DateTime<Utc>>,
    ) -> Subscription {
        Subscription {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: Uuid::nil(),
            plan: "pro".to_owned(),
            status: status.to_owned(),
            max_devices: 1,
            features: serde_json::json!([]),
            starts_at,
            expires_at,
            cancelled_at,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }
}
