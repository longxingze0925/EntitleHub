use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        application::repository::ApplicationRepository,
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        customer::repository::CustomerRepository,
        subscription::{
            model::{
                validate_subscription_status_filter, CreateSubscriptionInput, NewSubscription,
                Subscription, SubscriptionListMeta, SubscriptionListQuery, SubscriptionSummary,
            },
            repository::SubscriptionRepository,
        },
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct SubscriptionListResponse {
    pub items: Vec<SubscriptionSummary>,
    pub meta: SubscriptionListMeta,
}

#[derive(Debug, Serialize)]
pub struct CreateSubscriptionResponse {
    pub subscription: SubscriptionSummary,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionMutationResponse {
    pub subscription: SubscriptionSummary,
    pub revoked_sessions: u64,
}

pub async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<SubscriptionListQuery>,
) -> Result<Json<ApiResponse<SubscriptionListResponse>>, AppError> {
    ensure_admin_permission(&admin, "subscription:read")?;

    validate_subscription_status_filter(query.status.as_deref())?;
    let subscriptions = SubscriptionRepository::new(state.db.clone())
        .list(admin.tenant_id, &query)
        .await?;
    let items = subscriptions
        .into_iter()
        .map(SubscriptionSummary::from)
        .collect();

    Ok(Json(ApiResponse::ok(
        SubscriptionListResponse {
            items,
            meta: SubscriptionListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn create_subscription(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateSubscriptionInput>,
) -> Result<Json<ApiResponse<CreateSubscriptionResponse>>, AppError> {
    ensure_admin_permission(&admin, "subscription:create")?;
    ensure_application_exists(&state, admin.tenant_id, payload.app_id).await?;
    ensure_customer_exists(&state, admin.tenant_id, payload.customer_id).await?;

    let new_subscription = NewSubscription::from_input(admin.tenant_id, payload)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let subscription =
        create_subscription_in_transaction(&mut transaction, new_subscription).await?;
    audit_subscription_create(&mut transaction, &admin, &request_id, &subscription).await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CreateSubscriptionResponse {
            subscription: subscription.into(),
        },
        request_id.to_string(),
    )))
}

pub async fn cancel_subscription(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(subscription_id): Path<Uuid>,
) -> Result<Json<ApiResponse<SubscriptionMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "subscription:cancel")?;

    let repository = SubscriptionRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, subscription_id)
        .await?
        .ok_or_else(|| AppError::not_found("subscription not found"))?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let subscription =
        cancel_subscription_in_transaction(&mut transaction, admin.tenant_id, subscription_id)
            .await?
            .ok_or_else(|| AppError::conflict("subscription already cancelled"))?;
    let revoked_refresh_tokens = revoke_subscription_refresh_tokens_in_transaction(
        &mut transaction,
        admin.tenant_id,
        subscription_id,
    )
    .await?;
    let revoked_sessions = revoke_subscription_sessions_in_transaction(
        &mut transaction,
        admin.tenant_id,
        subscription_id,
    )
    .await?;
    audit_subscription_status_change(
        &mut transaction,
        &admin,
        &request_id,
        "subscription.cancel",
        &before,
        &subscription,
        revoked_sessions,
        revoked_refresh_tokens,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        SubscriptionMutationResponse {
            subscription: subscription.into(),
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

async fn create_subscription_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    subscription: NewSubscription,
) -> Result<Subscription, AppError> {
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
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn cancel_subscription_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    subscription_id: Uuid,
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
    .bind(subscription_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn revoke_subscription_sessions_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    subscription_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_sessions
        set revoked_at = now()
        where tenant_id = $1
          and revoked_at is null
          and device_id in (
            select id
            from devices
            where tenant_id = $1
              and subscription_id = $2
              and deleted_at is null
          )
        "#,
    )
    .bind(tenant_id)
    .bind(subscription_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_subscription_refresh_tokens_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    subscription_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_refresh_tokens rt
        set revoked_at = now()
        from client_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and rt.revoked_at is null
          and s.device_id in (
            select id
            from devices
            where tenant_id = $1
              and subscription_id = $2
              and deleted_at is null
          )
        "#,
    )
    .bind(tenant_id)
    .bind(subscription_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn ensure_application_exists(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
) -> Result<(), AppError> {
    ApplicationRepository::new(state.db.clone())
        .find_by_id(tenant_id, app_id)
        .await?
        .ok_or_else(AppError::app_not_found)?;

    Ok(())
}

async fn ensure_customer_exists(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    CustomerRepository::new(state.db.clone())
        .find_by_id(tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;

    Ok(())
}

async fn audit_subscription_create(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    subscription: &Subscription,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "subscription.create",
            resource_type: "subscription",
            resource_id: Some(subscription.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(subscription_audit_json(subscription)),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn audit_subscription_status_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: &Subscription,
    subscription: &Subscription,
    revoked_sessions: u64,
    revoked_refresh_tokens: u64,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "subscription",
            resource_id: Some(subscription.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(subscription_audit_json(before)),
            after_json: Some(subscription_audit_json(subscription)),
            metadata_json: json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await
}

fn subscription_audit_json(subscription: &Subscription) -> Value {
    json!({
        "id": subscription.id,
        "app_id": subscription.app_id,
        "customer_id": subscription.customer_id,
        "plan": subscription.plan,
        "status": subscription.status,
        "max_devices": subscription.max_devices,
        "features": subscription.features,
        "starts_at": subscription.starts_at,
        "expires_at": subscription.expires_at,
        "cancelled_at": subscription.cancelled_at,
        "metadata": subscription.metadata,
    })
}

fn ensure_admin_permission(admin: &AdminContext, permission_code: &str) -> Result<(), AppError> {
    if admin
        .permissions
        .iter()
        .any(|permission| permission == permission_code)
    {
        return Ok(());
    }

    Err(AppError::forbidden(format!(
        "missing permission: {permission_code}"
    )))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("subscription admin database error: {error}"))
}
