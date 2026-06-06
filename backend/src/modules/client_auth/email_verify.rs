use axum::{extract::State, http::HeaderMap, Extension, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::one_time_token::OneTimeToken,
        client_auth::session::ClientContext,
        customer::model::Customer,
        outbox,
    },
    rate_limit,
    state::AppState,
};

const CUSTOMER_EMAIL_VERIFY_PURPOSE: &str = "email_verify";
const CUSTOMER_EMAIL_VERIFY_TTL_HOURS: i64 = 24;

#[derive(Debug, Serialize)]
pub struct EmailVerifyRequestResponse {
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct EmailVerifyConfirmRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct EmailVerifyConfirmResponse {
    pub customer_id: Uuid,
    pub email_verified: bool,
}

pub async fn request_email_verify(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<EmailVerifyRequestResponse>>, AppError> {
    let customer_id = client
        .customer_id
        .ok_or_else(|| AppError::invalid_request("session has no customer"))?;
    let ip = rate_limit::client_ip(&headers);
    rate_limit::check_fixed_window(
        &state,
        rate_limit::email_verify_key(&customer_id.to_string(), &ip),
        state.config.security.login_rate_limit_max,
        state.config.security.login_rate_limit_window_seconds,
        AppError::login_rate_limited,
    )
    .await?;

    let customer = find_customer(&state, client.tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    if customer.email_verified {
        return Err(AppError::conflict("customer email already verified"));
    }

    let token = generate_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let expires_at = Utc::now() + Duration::hours(CUSTOMER_EMAIL_VERIFY_TTL_HOURS);

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let token_id = create_email_verify_token(
        &mut transaction,
        client.tenant_id,
        customer.id,
        &customer.email,
        &token_hash,
        expires_at,
    )
    .await?;
    let outbox_event_id = outbox::enqueue_customer_email_verify_email(
        &mut transaction,
        &state,
        client.tenant_id,
        &customer.email,
        &token,
        expires_at,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(client.tenant_id),
            actor_type: "customer",
            actor_id: Some(customer.id),
            action: "customer.email_verify.request",
            resource_type: "customer",
            resource_id: Some(customer.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: None,
            metadata_json: serde_json::json!({
                "one_time_token_id": token_id,
                "outbox_event_id": outbox_event_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        EmailVerifyRequestResponse { expires_at },
        request_id.to_string(),
    )))
}

pub async fn confirm_email_verify(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<EmailVerifyConfirmRequest>,
) -> Result<Json<ApiResponse<EmailVerifyConfirmResponse>>, AppError> {
    let token = normalize_token(&payload.token)?;
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let one_time_token =
        find_active_email_verify_token_for_update(&mut transaction, &token_hash).await?;
    let customer_id = one_time_token
        .subject_id
        .ok_or_else(|| AppError::token_invalid("email verify token invalid"))?;
    let tenant_id = one_time_token
        .tenant_id
        .ok_or_else(|| AppError::token_invalid("email verify token invalid"))?;
    let before = find_customer_for_update(&mut transaction, tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    let customer = mark_customer_email_verified(&mut transaction, tenant_id, customer_id).await?;
    consume_token(&mut transaction, one_time_token.id).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(tenant_id),
            actor_type: "customer",
            actor_id: Some(customer.id),
            action: "customer.email_verify.confirm",
            resource_type: "customer",
            resource_id: Some(customer.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(customer_audit_json(&before)),
            after_json: Some(customer_audit_json(&customer)),
            metadata_json: serde_json::json!({
                "one_time_token_id": one_time_token.id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        EmailVerifyConfirmResponse {
            customer_id: customer.id,
            email_verified: customer.email_verified,
        },
        request_id.to_string(),
    )))
}

async fn create_email_verify_token(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    email: &str,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, AppError> {
    let id = Uuid::new_v4();

    sqlx::query(
        r#"
        insert into one_time_tokens (
          id,
          tenant_id,
          purpose,
          subject_type,
          subject_id,
          email,
          token_hash,
          expires_at,
          metadata
        )
        values ($1, $2, $3, 'customer', $4, lower($5), $6, $7, '{}'::jsonb)
        "#,
    )
    .bind(id)
    .bind(tenant_id)
    .bind(CUSTOMER_EMAIL_VERIFY_PURPOSE)
    .bind(customer_id)
    .bind(email)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(id)
}

async fn find_active_email_verify_token_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    token_hash: &str,
) -> Result<OneTimeToken, AppError> {
    sqlx::query_as::<_, OneTimeToken>(
        r#"
        select
          id,
          tenant_id,
          purpose,
          subject_type,
          subject_id,
          email,
          token_hash,
          created_by,
          expires_at,
          consumed_at,
          revoked_at,
          metadata,
          created_at
        from one_time_tokens
        where purpose = $1
          and subject_type = 'customer'
          and token_hash = $2
          and expires_at > now()
          and consumed_at is null
          and revoked_at is null
        for update
        "#,
    )
    .bind(CUSTOMER_EMAIL_VERIFY_PURPOSE)
    .bind(token_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::token_invalid("email verify token invalid"))
}

async fn consume_token(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    token_id: Uuid,
) -> Result<(), AppError> {
    let consumed = sqlx::query_scalar::<_, Uuid>(
        r#"
        update one_time_tokens
        set consumed_at = now()
        where id = $1
          and expires_at > now()
          and consumed_at is null
          and revoked_at is null
        returning id
        "#,
    )
    .bind(token_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    if consumed.is_none() {
        return Err(AppError::token_invalid("email verify token invalid"));
    }

    Ok(())
}

async fn find_customer(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<Customer>, AppError> {
    sqlx::query_as::<_, Customer>(&customer_select_sql(
        "where tenant_id = $1 and id = $2 and deleted_at is null",
    ))
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_customer_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<Customer>, AppError> {
    sqlx::query_as::<_, Customer>(&format!(
        "{} for update",
        customer_select_sql("where tenant_id = $1 and id = $2 and deleted_at is null")
    ))
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn mark_customer_email_verified(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<Customer, AppError> {
    sqlx::query_as::<_, Customer>(
        r#"
        update customers
        set
          email_verified = true,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          email,
          password_hash,
          name,
          phone,
          company,
          status,
          email_verified,
          metadata,
          remark,
          last_login_at,
          last_login_ip::text as last_login_ip,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn customer_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
          id,
          tenant_id,
          email,
          password_hash,
          name,
          phone,
          company,
          status,
          email_verified,
          metadata,
          remark,
          last_login_at,
          last_login_ip::text as last_login_ip,
          created_at,
          updated_at,
          deleted_at
        from customers
        {where_clause}
        "#
    )
}

fn customer_audit_json(customer: &Customer) -> serde_json::Value {
    serde_json::json!({
        "id": customer.id,
        "email": customer.email,
        "status": customer.status,
        "email_verified": customer.email_verified,
    })
}

fn normalize_token(token: &str) -> Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::invalid_request("token is required"));
    }

    Ok(token.to_owned())
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("customer email verify database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::normalize_token;

    #[test]
    fn normalize_token_trims_value() {
        assert_eq!(normalize_token(" token ").expect("token"), "token");
    }

    #[test]
    fn normalize_token_rejects_blank() {
        assert!(normalize_token(" ").is_err());
    }
}
