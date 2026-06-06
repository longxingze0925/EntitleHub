use axum::{extract::State, Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::{password::hash_password, token::hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::{one_time_token::OneTimeToken, password::validate_new_password},
        customer::model::Customer,
    },
    state::AppState,
};

const CUSTOMER_PASSWORD_RESET_PURPOSE: &str = "customer_password_reset";

#[derive(Debug, Deserialize)]
pub struct CustomerPasswordResetConfirmRequest {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct CustomerPasswordResetConfirmResponse {
    pub ok: bool,
    pub revoked_sessions: u64,
    pub revoked_refresh_tokens: u64,
}

pub async fn confirm_password_reset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CustomerPasswordResetConfirmRequest>,
) -> Result<Json<ApiResponse<CustomerPasswordResetConfirmResponse>>, AppError> {
    let token = normalize_token(&payload.token)?;
    validate_new_password(&payload.new_password)?;
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let password_hash = hash_password(&payload.new_password)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let one_time_token =
        find_active_password_reset_token_for_update(&mut transaction, &token_hash).await?;
    let customer_id = one_time_token
        .subject_id
        .ok_or_else(AppError::password_reset_token_invalid)?;
    let tenant_id = one_time_token
        .tenant_id
        .ok_or_else(AppError::password_reset_token_invalid)?;
    let before = find_customer_for_update(&mut transaction, tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    let customer = update_customer_password_in_transaction(
        &mut transaction,
        tenant_id,
        customer_id,
        &password_hash,
    )
    .await?;
    consume_token(&mut transaction, one_time_token.id).await?;
    let revoked_sessions =
        revoke_customer_sessions_in_transaction(&mut transaction, tenant_id, customer_id).await?;
    let revoked_refresh_tokens =
        revoke_customer_refresh_tokens_in_transaction(&mut transaction, tenant_id, customer_id)
            .await?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(tenant_id),
            actor_type: "customer",
            actor_id: Some(customer_id),
            action: "customer.password_reset.confirm",
            resource_type: "customer",
            resource_id: Some(customer_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(customer_audit_json(&before)),
            after_json: Some(serde_json::json!({
                "id": customer.id,
                "email": customer.email,
                "password_changed": true,
            })),
            metadata_json: serde_json::json!({
                "one_time_token_id": one_time_token.id,
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CustomerPasswordResetConfirmResponse {
            ok: true,
            revoked_sessions,
            revoked_refresh_tokens,
        },
        request_id.to_string(),
    )))
}

async fn find_active_password_reset_token_for_update(
    transaction: &mut Transaction<'_, Postgres>,
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
          and tenant_id is not null
          and subject_id is not null
          and email is not null
          and expires_at > now()
          and consumed_at is null
          and revoked_at is null
        for update
        "#,
    )
    .bind(CUSTOMER_PASSWORD_RESET_PURPOSE)
    .bind(token_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::password_reset_token_invalid)
}

async fn find_customer_for_update(
    transaction: &mut Transaction<'_, Postgres>,
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

async fn update_customer_password_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    password_hash: &str,
) -> Result<Customer, AppError> {
    sqlx::query_as::<_, Customer>(
        r#"
        update customers
        set
          password_hash = $3,
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
    .bind(password_hash)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn consume_token(
    transaction: &mut Transaction<'_, Postgres>,
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
        return Err(AppError::password_reset_token_invalid());
    }

    Ok(())
}

async fn revoke_customer_sessions_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_sessions
        set revoked_at = now()
        where tenant_id = $1
          and customer_id = $2
          and revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_customer_refresh_tokens_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_refresh_tokens rt
        set revoked_at = now()
        from client_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and s.customer_id = $2
          and rt.used_at is null
          and rt.revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
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
    AppError::dependency(format!("customer password reset database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::normalize_token;

    #[test]
    fn normalize_token_trims_value() {
        assert_eq!(
            normalize_token(" reset-token ").expect("token"),
            "reset-token"
        );
    }

    #[test]
    fn normalize_token_rejects_blank() {
        assert!(normalize_token(" ").is_err());
    }
}
