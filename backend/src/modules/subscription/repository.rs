use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::subscription::model::{NewSubscription, Subscription, SubscriptionListQuery},
};

#[derive(Clone)]
pub struct SubscriptionRepository {
    pool: PgPool,
}

impl SubscriptionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        query: &SubscriptionListQuery,
    ) -> Result<Vec<Subscription>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;
        let keyword = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let keyword_pattern = keyword.map(|value| format!("%{}%", value.to_lowercase()));
        let status = query
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let include_history = query.include_history.unwrap_or(false);

        sqlx::query_as::<_, Subscription>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              plan,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              cancelled_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from subscriptions
            where tenant_id = $1
              and deleted_at is null
              and ($2::uuid is null or app_id = $2)
              and ($3::uuid is null or customer_id = $3)
              and ($4::text is null or status = $4)
              and (
                $4::text is not null
                or $6::bool
                or (
                  status not in ('cancelled', 'expired')
                  and (expires_at is null or expires_at > now())
                )
              )
              and (
                $5::text is null
                or lower(plan) like $5
                or id::text like $5
              )
            order by created_at desc, id
            limit $7 offset $8
            "#,
        )
        .bind(tenant_id)
        .bind(query.app_id)
        .bind(query.customer_id)
        .bind(status)
        .bind(keyword_pattern)
        .bind(include_history)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn create(&self, subscription: NewSubscription) -> Result<Subscription, AppError> {
        sqlx::query_as::<_, Subscription>(
            r#"
            insert into subscriptions (
              id,
              tenant_id,
              app_id,
              customer_id,
              plan,
              max_devices,
              features,
              starts_at,
              expires_at,
              metadata
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              plan,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              cancelled_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(subscription.id)
        .bind(subscription.tenant_id)
        .bind(subscription.app_id)
        .bind(subscription.customer_id)
        .bind(subscription.plan)
        .bind(subscription.max_devices)
        .bind(subscription.features)
        .bind(subscription.starts_at)
        .bind(subscription.expires_at)
        .bind(subscription.metadata)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Subscription>, AppError> {
        sqlx::query_as::<_, Subscription>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              plan,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              cancelled_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from subscriptions
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_active_for_customer(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        customer_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<Subscription>, AppError> {
        sqlx::query_as::<_, Subscription>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              plan,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              cancelled_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from subscriptions
            where tenant_id = $1
              and app_id = $2
              and customer_id = $3
              and deleted_at is null
              and status in ('active', 'trialing')
              and cancelled_at is null
              and starts_at <= $4
              and (expires_at is null or expires_at > $4)
            order by expires_at nulls last, created_at desc, id
            limit 1
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(customer_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn cancel(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Subscription>, AppError> {
        sqlx::query_as::<_, Subscription>(
            r#"
            update subscriptions
            set
              status = 'cancelled',
              cancelled_at = now(),
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
              and status <> 'cancelled'
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              plan,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              cancelled_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("subscription repository database error: {error}"))
}
