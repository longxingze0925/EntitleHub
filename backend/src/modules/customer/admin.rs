use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::{
        password::hash_password,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::{password::validate_new_password, session::AdminContext},
        customer::{
            model::{Customer, CustomerListMeta, CustomerListQuery, NewCustomer, UpdateCustomer},
            repository::CustomerRepository,
        },
        outbox,
    },
    state::AppState,
};

const CUSTOMER_PASSWORD_RESET_PURPOSE: &str = "customer_password_reset";
const CUSTOMER_PASSWORD_RESET_TTL_HOURS: i64 = 2;

#[derive(Debug, Serialize)]
pub struct CustomerListResponse {
    pub items: Vec<CustomerResponse>,
    pub meta: CustomerListMeta,
}

#[derive(Debug, Serialize)]
pub struct CustomerResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub status: String,
    pub email_verified: bool,
    pub metadata: Value,
    pub remark: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCustomerRequest {
    pub email: String,
    pub name: Option<String>,
    pub password: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub metadata: Option<Value>,
    pub remark: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCustomerRequest {
    pub name: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub metadata: Option<Value>,
    pub remark: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CustomerMutationResponse {
    pub customer: CustomerResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revoked_sessions: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct CustomerPasswordResetResponse {
    pub expires_at: chrono::DateTime<Utc>,
}

pub async fn list_customers(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<CustomerListQuery>,
) -> Result<Json<ApiResponse<CustomerListResponse>>, AppError> {
    ensure_admin_permission(&admin, "customer:read")?;

    validate_customer_status_filter(query.status.as_deref())?;
    let customers = CustomerRepository::new(state.db.clone())
        .list(admin.tenant_id, &query)
        .await?;
    let items = customers.into_iter().map(customer_response).collect();

    Ok(Json(ApiResponse::ok(
        CustomerListResponse {
            items,
            meta: CustomerListMeta {
                page: query.page.unwrap_or(1).max(1),
                page_size: query.page_size.unwrap_or(20).clamp(1, 100),
            },
        },
        request_id.to_string(),
    )))
}

pub async fn create_customer(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateCustomerRequest>,
) -> Result<Json<ApiResponse<CustomerMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "customer:create")?;

    let email = normalize_email(&payload.email)?;
    let repository = CustomerRepository::new(state.db.clone());
    if repository
        .find_by_email(admin.tenant_id, &email)
        .await?
        .is_some()
    {
        return Err(AppError::duplicate_email());
    }

    let password_hash = clean_optional(payload.password)
        .map(|password| {
            validate_new_password(&password)?;
            hash_password(&password)
        })
        .transpose()?;
    let new_customer = NewCustomer::new(
        admin.tenant_id,
        email,
        password_hash,
        clean_optional(payload.name),
        clean_optional(payload.phone),
        clean_optional(payload.company),
        payload.metadata.unwrap_or_else(|| json!({})),
        clean_optional(payload.remark),
    );
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let customer = create_customer_in_transaction(&mut transaction, new_customer).await?;
    audit_customer_create(&mut transaction, &admin, &request_id, &customer).await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CustomerMutationResponse {
            customer: customer_response(customer),
            revoked_sessions: None,
        },
        request_id.to_string(),
    )))
}

pub async fn update_customer(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
    Json(payload): Json<UpdateCustomerRequest>,
) -> Result<Json<ApiResponse<CustomerMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "customer:update")?;

    let repository = CustomerRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    let input = UpdateCustomer {
        name: clean_optional(payload.name),
        phone: clean_optional(payload.phone),
        company: clean_optional(payload.company),
        metadata: payload.metadata,
        remark: clean_optional(payload.remark),
    };
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let customer =
        update_customer_in_transaction(&mut transaction, admin.tenant_id, customer_id, input)
            .await?
            .ok_or_else(|| AppError::not_found("customer not found"))?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "customer.update",
            resource_type: "customer",
            resource_id: Some(customer.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(customer_audit_json(&before)),
            after_json: Some(customer_audit_json(&customer)),
            metadata_json: json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CustomerMutationResponse {
            customer: customer_response(customer),
            revoked_sessions: None,
        },
        request_id.to_string(),
    )))
}

pub async fn disable_customer(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
) -> Result<Json<ApiResponse<CustomerMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "customer:disable")?;

    let repository = CustomerRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let customer = disable_customer_in_transaction(&mut transaction, admin.tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::conflict("customer already disabled"))?;
    let revoked_refresh_tokens = revoke_customer_refresh_tokens_in_transaction(
        &mut transaction,
        admin.tenant_id,
        customer_id,
    )
    .await?;
    let revoked_sessions =
        revoke_customer_sessions_in_transaction(&mut transaction, admin.tenant_id, customer_id)
            .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "customer.disable",
            resource_type: "customer",
            resource_id: Some(customer.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "status": before.status,
            })),
            after_json: Some(json!({
                "status": customer.status,
            })),
            metadata_json: json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CustomerMutationResponse {
            customer: customer_response(customer),
            revoked_sessions: Some(revoked_sessions),
        },
        request_id.to_string(),
    )))
}

pub async fn reset_customer_password(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
) -> Result<Json<ApiResponse<CustomerPasswordResetResponse>>, AppError> {
    ensure_admin_permission(&admin, "customer:reset_password")?;

    let repository = CustomerRepository::new(state.db.clone());
    let customer = repository
        .find_by_id(admin.tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;
    let token = generate_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let expires_at = Utc::now() + Duration::hours(CUSTOMER_PASSWORD_RESET_TTL_HOURS);

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let token_id = create_password_reset_token(
        &mut transaction,
        admin.tenant_id,
        customer.id,
        &customer.email,
        &token_hash,
        admin.team_member_id,
        expires_at,
    )
    .await?;
    let outbox_event_id = outbox::enqueue_customer_password_reset_email(
        &mut transaction,
        &state,
        admin.tenant_id,
        &customer.email,
        &token,
        expires_at,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "customer.password_reset.create",
            resource_type: "customer",
            resource_id: Some(customer.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: None,
            metadata_json: json!({
                "one_time_token_id": token_id,
                "outbox_event_id": outbox_event_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        CustomerPasswordResetResponse { expires_at },
        request_id.to_string(),
    )))
}

async fn create_customer_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    customer: NewCustomer,
) -> Result<Customer, AppError> {
    sqlx::query_as::<_, Customer>(
        r#"
        insert into customers (
          id,
          tenant_id,
          email,
          password_hash,
          name,
          phone,
          company,
          metadata,
          remark
        )
        values ($1, $2, lower($3), $4, $5, $6, $7, $8, $9)
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
    .bind(customer.id)
    .bind(customer.tenant_id)
    .bind(customer.email)
    .bind(customer.password_hash)
    .bind(customer.name)
    .bind(customer.phone)
    .bind(customer.company)
    .bind(customer.metadata)
    .bind(customer.remark)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_customer_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    input: UpdateCustomer,
) -> Result<Option<Customer>, AppError> {
    sqlx::query_as::<_, Customer>(
        r#"
        update customers
        set
          name = coalesce($3, name),
          phone = coalesce($4, phone),
          company = coalesce($5, company),
          metadata = coalesce($6, metadata),
          remark = coalesce($7, remark),
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
    .bind(input.name)
    .bind(input.phone)
    .bind(input.company)
    .bind(input.metadata)
    .bind(input.remark)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn disable_customer_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<Option<Customer>, AppError> {
    sqlx::query_as::<_, Customer>(
        r#"
        update customers
        set
          status = 'disabled',
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
          and status <> 'disabled'
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
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
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

fn customer_response(customer: Customer) -> CustomerResponse {
    CustomerResponse {
        id: customer.id,
        email: customer.email,
        name: customer.name,
        phone: customer.phone,
        company: customer.company,
        status: customer.status,
        email_verified: customer.email_verified,
        metadata: customer.metadata,
        remark: customer.remark,
    }
}

fn customer_audit_json(customer: &Customer) -> Value {
    json!({
        "id": customer.id,
        "email": customer.email,
        "name": customer.name,
        "phone": customer.phone,
        "company": customer.company,
        "status": customer.status,
        "email_verified": customer.email_verified,
        "metadata": customer.metadata,
        "remark": customer.remark,
    })
}

async fn audit_customer_create(
    transaction: &mut Transaction<'_, Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    customer: &Customer,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "customer.create",
            resource_type: "customer",
            resource_id: Some(customer.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(customer_audit_json(customer)),
            metadata_json: json!({}),
        },
    )
    .await
}

async fn create_password_reset_token(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    email: &str,
    token_hash: &str,
    created_by: Uuid,
    expires_at: chrono::DateTime<Utc>,
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
          created_by,
          expires_at,
          metadata
        )
        values ($1, $2, $3, 'customer', $4, lower($5), $6, $7, $8, '{}'::jsonb)
        "#,
    )
    .bind(id)
    .bind(tenant_id)
    .bind(CUSTOMER_PASSWORD_RESET_PURPOSE)
    .bind(customer_id)
    .bind(email)
    .bind(token_hash)
    .bind(created_by)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(id)
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

fn normalize_email(email: &str) -> Result<String, AppError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::validation_failed("email is invalid"));
    }

    Ok(email)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn validate_customer_status_filter(status: Option<&str>) -> Result<(), AppError> {
    let Some(status) = status else {
        return Ok(());
    };

    if matches!(status, "active" | "disabled" | "banned" | "pending") {
        return Ok(());
    }

    Err(AppError::validation_failed("customer status is invalid"))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("customer admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{clean_optional, normalize_email, validate_customer_status_filter};

    #[test]
    fn normalize_email_trims_and_lowercases() {
        assert_eq!(
            normalize_email(" User@Example.COM ").expect("email should normalize"),
            "user@example.com"
        );
    }

    #[test]
    fn clean_optional_turns_blank_into_none() {
        assert_eq!(clean_optional(Some("  ".to_owned())), None);
        assert_eq!(
            clean_optional(Some(" Acme ".to_owned())),
            Some("Acme".to_owned())
        );
    }

    #[test]
    fn status_filter_accepts_known_values() {
        assert!(validate_customer_status_filter(Some("active")).is_ok());
        assert!(validate_customer_status_filter(Some("unknown")).is_err());
    }
}
