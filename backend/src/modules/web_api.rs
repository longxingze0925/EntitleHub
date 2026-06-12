use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    crypto::password::{hash_password, verify_password},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        ai::capabilities,
        auth::password::validate_new_password,
        customer::{
            model::{Customer, NewCustomer},
            repository::CustomerRepository,
        },
        server_api::{ai_invoke_scope, authenticate_server_key, ServerApiKeyContext},
        subscription::{model::SubscriptionSummary, repository::SubscriptionRepository},
    },
    state::AppState,
};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Deserialize)]
pub struct WebCustomerRegisterRequest {
    pub email: String,
    pub password: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebCustomerLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct WebCustomerAuthResponse {
    pub user: WebCustomerUser,
}

#[derive(Debug, Serialize)]
pub struct WebCustomerResponse {
    pub user: WebCustomerUser,
}

#[derive(Debug, Serialize)]
pub struct WebCustomerUser {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    #[serde(rename = "customerId")]
    pub customer_id: Uuid,
    pub status: String,
    pub email_verified: bool,
}

#[derive(Debug, Serialize, FromRow)]
pub struct WebCustomerBalance {
    pub customer_id: Uuid,
    pub currency: String,
    pub balance_minor: i64,
    pub held_minor: i64,
    pub available_minor: i64,
    pub ai_enabled: bool,
    pub daily_spend_limit_minor: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct WebCustomerBalanceResponse {
    pub balance: WebCustomerBalance,
}

#[derive(Debug, Deserialize)]
pub struct WebCustomerUsageQuery {
    pub status: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WebAiModelListQuery {
    #[serde(rename = "type")]
    pub modality: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct WebAiModelSummary {
    code: String,
    name: String,
    modality: String,
    provider_model: Option<String>,
    currency: String,
    billing_mode: String,
    input_1k_price_minor: i64,
    output_1k_price_minor: i64,
    request_price_minor: i64,
    image_price_minor: i64,
    second_price_minor: i64,
    minute_price_minor: i64,
    pricing_config: serde_json::Value,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct WebCustomerUsageRecord {
    pub id: Uuid,
    pub endpoint: String,
    pub status: String,
    pub provider_status: Option<String>,
    pub provider_request_id: Option<String>,
    pub model_code: Option<String>,
    pub provider_name: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub charged_minor: i64,
    pub refunded_minor: i64,
    pub currency: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct WebCustomerUsageResponse {
    pub items: Vec<WebCustomerUsageRecord>,
    pub meta: WebListMeta,
}

#[derive(Debug, Serialize)]
pub struct WebCustomerPlanResponse {
    pub plan: Option<SubscriptionSummary>,
}

#[derive(Debug, Serialize)]
pub struct WebListMeta {
    pub page: i64,
    pub page_size: i64,
}

pub async fn register_customer(
    State(state): State<AppState>,
    request_id: axum::Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<WebCustomerRegisterRequest>,
) -> Result<Json<ApiResponse<WebCustomerAuthResponse>>, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    let email = normalize_email(&payload.email)?;
    validate_new_password(&payload.password)?;
    let name = clean_optional(payload.name);
    let repository = CustomerRepository::new(state.db.clone());

    if repository
        .find_by_email(server_key.tenant_id, &email)
        .await?
        .is_some()
    {
        return Err(AppError::duplicate_email());
    }

    let password_hash = hash_password(&payload.password)?;
    let customer = repository
        .create(NewCustomer::new(
            server_key.tenant_id,
            email,
            Some(password_hash),
            name,
            None,
            None,
            json!({ "source": "server_web_api" }),
            None,
        ))
        .await?;

    Ok(Json(ApiResponse::ok(
        WebCustomerAuthResponse {
            user: web_customer_user(customer),
        },
        request_id.0.to_string(),
    )))
}

pub async fn login_customer(
    State(state): State<AppState>,
    request_id: axum::Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<WebCustomerLoginRequest>,
) -> Result<Json<ApiResponse<WebCustomerAuthResponse>>, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    let email = normalize_email(&payload.email)?;
    let customer = CustomerRepository::new(state.db.clone())
        .find_by_email(server_key.tenant_id, &email)
        .await?
        .ok_or_else(AppError::invalid_credentials)?;

    if customer.status != "active" {
        return Err(AppError::account_disabled());
    }

    let password_hash = customer
        .password_hash
        .as_deref()
        .ok_or_else(AppError::invalid_credentials)?;
    if !verify_password(&payload.password, password_hash)? {
        return Err(AppError::invalid_credentials());
    }

    update_customer_last_login(&state, server_key.tenant_id, customer.id).await?;
    let customer = CustomerRepository::new(state.db.clone())
        .find_by_id(server_key.tenant_id, customer.id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))?;

    Ok(Json(ApiResponse::ok(
        WebCustomerAuthResponse {
            user: web_customer_user(customer),
        },
        request_id.0.to_string(),
    )))
}

pub async fn get_customer(
    State(state): State<AppState>,
    request_id: axum::Extension<RequestId>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
) -> Result<Json<ApiResponse<WebCustomerResponse>>, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    let customer = load_customer(&state, server_key.tenant_id, customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebCustomerResponse {
            user: web_customer_user(customer),
        },
        request_id.0.to_string(),
    )))
}

pub async fn get_customer_balance(
    State(state): State<AppState>,
    request_id: axum::Extension<RequestId>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
) -> Result<Json<ApiResponse<WebCustomerBalanceResponse>>, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    ensure_customer_exists(&state, server_key.tenant_id, customer_id).await?;
    let balance = load_customer_balance(&state, server_key.tenant_id, customer_id).await?;

    Ok(Json(ApiResponse::ok(
        WebCustomerBalanceResponse { balance },
        request_id.0.to_string(),
    )))
}

pub async fn get_customer_usage(
    State(state): State<AppState>,
    request_id: axum::Extension<RequestId>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
    Query(query): Query<WebCustomerUsageQuery>,
) -> Result<Json<ApiResponse<WebCustomerUsageResponse>>, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    ensure_customer_exists(&state, server_key.tenant_id, customer_id).await?;
    let status = query
        .status
        .as_deref()
        .map(normalize_usage_status)
        .transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_customer_usage(
        &state,
        server_key.tenant_id,
        customer_id,
        status.as_deref(),
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        WebCustomerUsageResponse {
            items,
            meta: WebListMeta { page, page_size },
        },
        request_id.0.to_string(),
    )))
}

pub async fn get_customer_plan(
    State(state): State<AppState>,
    request_id: axum::Extension<RequestId>,
    headers: HeaderMap,
    Path(customer_id): Path<Uuid>,
) -> Result<Json<ApiResponse<WebCustomerPlanResponse>>, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    ensure_customer_exists(&state, server_key.tenant_id, customer_id).await?;
    let plan = SubscriptionRepository::new(state.db.clone())
        .find_active_for_customer(
            server_key.tenant_id,
            server_key.app_id,
            customer_id,
            Utc::now(),
        )
        .await?
        .map(SubscriptionSummary::from);

    Ok(Json(ApiResponse::ok(
        WebCustomerPlanResponse { plan },
        request_id.0.to_string(),
    )))
}

pub async fn list_ai_models(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<WebAiModelListQuery>,
) -> Result<axum::response::Response, AppError> {
    let server_key = authenticate_web_server_key(&state, &headers).await?;
    let modality = query
        .modality
        .as_deref()
        .map(normalize_modality)
        .transpose()?;
    let models = sqlx::query_as::<_, WebAiModelSummary>(
        r#"
        select
          m.code,
          m.name,
          m.modality,
          m.provider_model,
          m.currency,
          m.billing_mode,
          m.input_1k_price_minor,
          m.output_1k_price_minor,
          m.request_price_minor,
          m.image_price_minor,
          m.second_price_minor,
          m.minute_price_minor,
          m.pricing_config_json as pricing_config,
          m.created_at
        from ai_models m
        join ai_providers p
          on p.id = m.provider_id
          and p.tenant_id = m.tenant_id
        where m.tenant_id = $1
          and m.enabled
          and p.enabled
          and ($2::text is null or m.modality = $2)
        order by m.code
        "#,
    )
    .bind(server_key.tenant_id)
    .bind(modality)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)?;
    let data = models
        .into_iter()
        .map(|model| {
            let capabilities = capabilities::model_capabilities_json(&model.pricing_config)?;
            Ok(json!({
                "id": model.code,
                "object": "model",
                "created": model.created_at.timestamp(),
                "owned_by": "entitlehub",
                "name": model.name,
                "modality": model.modality,
                "provider_model": model.provider_model,
                "billing": {
                    "currency": model.currency,
                    "mode": model.billing_mode,
                    "input_1k_price_minor": model.input_1k_price_minor,
                    "output_1k_price_minor": model.output_1k_price_minor,
                    "request_price_minor": model.request_price_minor,
                    "image_price_minor": model.image_price_minor,
                    "second_price_minor": model.second_price_minor,
                    "minute_price_minor": model.minute_price_minor,
                },
                "capabilities": capabilities,
            }))
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok((
        axum::http::StatusCode::OK,
        Json(json!({ "object": "list", "data": data })),
    )
        .into_response())
}

async fn authenticate_web_server_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<ServerApiKeyContext, AppError> {
    authenticate_server_key(state, headers, ai_invoke_scope()).await
}

async fn load_customer(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<Customer, AppError> {
    CustomerRepository::new(state.db.clone())
        .find_by_id(tenant_id, customer_id)
        .await?
        .ok_or_else(|| AppError::not_found("customer not found"))
}

async fn ensure_customer_exists(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    load_customer(state, tenant_id, customer_id).await?;
    Ok(())
}

async fn load_customer_balance(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<WebCustomerBalance, AppError> {
    let balance = sqlx::query_as::<_, WebCustomerBalance>(
        r#"
        select
          customer_id,
          currency,
          balance_minor,
          held_minor,
          greatest(balance_minor - held_minor, 0)::bigint as available_minor,
          ai_enabled,
          daily_spend_limit_minor
        from ai_wallets
        where tenant_id = $1
          and customer_id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(balance.unwrap_or(WebCustomerBalance {
        customer_id,
        currency: "CNY".to_owned(),
        balance_minor: 0,
        held_minor: 0,
        available_minor: 0,
        ai_enabled: true,
        daily_spend_limit_minor: None,
    }))
}

async fn list_customer_usage(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    status: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<WebCustomerUsageRecord>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, WebCustomerUsageRecord>(
        r#"
        select
          u.id,
          u.endpoint,
          u.status,
          u.provider_status,
          u.provider_request_id,
          m.code as model_code,
          p.name as provider_name,
          u.prompt_tokens,
          u.completion_tokens,
          u.total_tokens,
          u.charged_minor,
          u.refunded_minor,
          coalesce(m.currency, 'CNY') as currency,
          u.created_at,
          u.completed_at
        from ai_usage_records u
        left join ai_models m
          on m.id = u.model_id
          and m.tenant_id = u.tenant_id
        left join ai_providers p
          on p.id = u.provider_id
          and p.tenant_id = u.tenant_id
        where u.tenant_id = $1
          and u.customer_id = $2
          and ($3::text is null or u.status = $3)
        order by u.created_at desc, u.id desc
        limit $4 offset $5
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(status)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn update_customer_last_login(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update customers
        set last_login_at = now(),
            updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

fn web_customer_user(customer: Customer) -> WebCustomerUser {
    WebCustomerUser {
        id: customer.id,
        email: customer.email,
        name: customer.name,
        customer_id: customer.id,
        status: customer.status,
        email_verified: customer.email_verified,
    }
}

fn normalize_email(email: &str) -> Result<String, AppError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') || email.len() > 320 || email.contains('\0') {
        return Err(AppError::validation_failed("email is invalid"));
    }

    Ok(email)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn normalize_usage_status(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "pending" | "running" | "succeeded" | "failed" | "refunded" => Ok(value),
        _ => Err(AppError::validation_failed("ai usage status is invalid")),
    }
}

fn normalize_modality(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "text" | "image" | "video" | "audio" | "embedding" | "multimodal" => Ok(value),
        _ => Err(AppError::validation_failed("ai model type is invalid")),
    }
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("web api database error: {error}"))
}
