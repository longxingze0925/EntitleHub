use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, Response as HttpResponse, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::header::{
    HeaderMap as ReqwestHeaderMap, HeaderName as ReqwestHeaderName,
    HeaderValue as ReqwestHeaderValue, AUTHORIZATION,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, PrivateKeyEnvelope},
    error::AppError,
    http::request_id::RequestId,
    metrics::{self, AiGatewayRequestStatus},
    modules::{
        ai::api_keys::{authenticate_api_key, AiApiKeyContext},
        client_auth::session::ClientContext,
    },
    rate_limit,
    state::AppState,
};

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_COMPLETION_TOKEN_BUDGET: i64 = 4096;
const MAX_COMPLETION_TOKEN_BUDGET: i64 = 128_000;
const MAX_PROMPT_TOKEN_ESTIMATE: i64 = 512_000;
const MAX_IMAGE_COUNT: i64 = 10;
const MAX_AI_ASSET_BYTES: u64 = 50 * 1024 * 1024;
const MAX_IDEMPOTENCY_KEY_LEN: usize = 200;

#[derive(Debug, Clone, FromRow)]
struct GatewayModel {
    id: Uuid,
    provider_id: Uuid,
    provider_kind: String,
    provider_name: String,
    provider_base_url: String,
    provider_config: Value,
    provider_secret_encrypted: Option<String>,
    code: String,
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
    daily_spend_limit_minor: Option<i64>,
    pricing_config: Value,
}

#[derive(Debug, Clone, FromRow)]
struct WalletRecord {
    id: Uuid,
    tenant_id: Uuid,
    customer_id: Uuid,
    currency: String,
    balance_minor: i64,
    held_minor: i64,
    ai_enabled: bool,
    daily_spend_limit_minor: Option<i64>,
}

#[derive(Debug, Clone)]
struct BillingReservation {
    usage_id: Uuid,
    wallet_id: Uuid,
    held_minor: i64,
}

#[derive(Debug, Clone, Copy)]
struct TokenUsage {
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    total_tokens: Option<i64>,
}

impl Default for TokenUsage {
    fn default() -> Self {
        Self {
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: None,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
struct AiAssetRecord {
    storage_key: String,
    mime_type: Option<String>,
    file_size: Option<i64>,
}

#[derive(Debug, Clone)]
struct GatewayCaller {
    tenant_id: Uuid,
    customer_id: Uuid,
    api_key_id: Option<Uuid>,
    rate_limit_key: String,
    api_key_daily_spend_limit_minor: Option<i64>,
    source: &'static str,
}

impl GatewayCaller {
    fn from_api_key(api_key: AiApiKeyContext) -> Self {
        Self {
            tenant_id: api_key.tenant_id,
            customer_id: api_key.customer_id,
            api_key_id: Some(api_key.api_key_id),
            rate_limit_key: format!("api_key:{}", api_key.api_key_id),
            api_key_daily_spend_limit_minor: api_key.daily_spend_limit_minor,
            source: "api_key",
        }
    }

    fn from_client_context(client: &ClientContext) -> Result<Self, AppError> {
        let customer_id = client.customer_id.ok_or_else(|| {
            AppError::business_rule_failed("client session is not linked to a customer")
        })?;

        Ok(Self {
            tenant_id: client.tenant_id,
            customer_id,
            api_key_id: None,
            rate_limit_key: format!("client:{}:{}", customer_id, client.device_id),
            api_key_daily_spend_limit_minor: None,
            source: "client_session",
        })
    }
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let caller = GatewayCaller::from_api_key(authenticate_api_key(&state, &headers).await?);
    chat_completions_for_caller(state, request_id, headers, payload, caller).await
}

pub async fn client_chat_completions(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Extension(client): Extension<ClientContext>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let caller = GatewayCaller::from_client_context(&client)?;
    chat_completions_for_caller(state, request_id, headers, payload, caller).await
}

async fn chat_completions_for_caller(
    state: AppState,
    request_id: RequestId,
    headers: HeaderMap,
    payload: Value,
    caller: GatewayCaller,
) -> Result<Response, AppError> {
    check_gateway_rate_limit(&state, &caller).await?;
    let endpoint = "/v1/chat/completions";
    let idempotency_key = idempotency_key(&headers)?;
    if let Some(response) =
        find_idempotent_response(&state, &caller, endpoint, idempotency_key.as_deref()).await?
    {
        return Ok(response);
    }
    reject_streaming_request(&payload)?;
    let model_code = requested_model_code(&payload)?;
    let model = load_gateway_model(&state, caller.tenant_id, model_code).await?;
    validate_model_for_chat(&model)?;

    let hold_minor = estimate_hold_minor(&payload, &model)?;
    let reservation = reserve_wallet_and_create_usage(
        &state,
        &caller,
        &model,
        &request_id.to_string(),
        endpoint,
        hold_minor,
        &payload,
        idempotency_key.as_deref(),
    )
    .await?;

    let provider_started = Instant::now();
    let provider_result =
        forward_openai_compatible_json(&state, &model, payload, "chat/completions").await;
    metrics::record_ai_gateway_provider_duration(endpoint, provider_started.elapsed());

    match provider_result {
        Ok(provider_response) if provider_response.status.is_success() => {
            let usage = token_usage_from_response(&provider_response.body);
            let charge_minor = calculate_actual_charge_minor(&model, usage)
                .unwrap_or(reservation.held_minor)
                .min(reservation.held_minor);
            capture_usage(
                &state,
                &reservation,
                &model,
                provider_response.status.as_u16() as i32,
                provider_response.provider_request_id.as_deref(),
                usage,
                charge_minor,
                &provider_response.body,
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Success);
            metrics::record_ai_gateway_charged(endpoint, charge_minor);

            Ok(provider_json_response(
                provider_response.status,
                provider_response.body,
                reservation.usage_id,
            ))
        }
        Ok(provider_response) => {
            release_usage(
                &state,
                &reservation,
                provider_response.status.as_u16() as i32,
                provider_response.provider_request_id.as_deref(),
                Some(&provider_response.body),
                "AI 请求失败，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::ProviderError);

            Ok(provider_json_response(
                provider_response.status,
                provider_response.body,
                reservation.usage_id,
            ))
        }
        Err(error) => {
            release_usage(
                &state,
                &reservation,
                0,
                None,
                None,
                "AI 请求异常，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Error);

            Err(error)
        }
    }
}

pub async fn image_generations(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let caller = GatewayCaller::from_api_key(authenticate_api_key(&state, &headers).await?);
    image_generations_for_caller(state, request_id, headers, payload, caller).await
}

pub async fn client_image_generations(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Extension(client): Extension<ClientContext>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let caller = GatewayCaller::from_client_context(&client)?;
    image_generations_for_caller(state, request_id, headers, payload, caller).await
}

async fn image_generations_for_caller(
    state: AppState,
    request_id: RequestId,
    headers: HeaderMap,
    payload: Value,
    caller: GatewayCaller,
) -> Result<Response, AppError> {
    check_gateway_rate_limit(&state, &caller).await?;
    let endpoint = "/v1/images/generations";
    let idempotency_key = idempotency_key(&headers)?;
    if let Some(response) =
        find_idempotent_response(&state, &caller, endpoint, idempotency_key.as_deref()).await?
    {
        return Ok(response);
    }
    let model_code = requested_model_code(&payload)?;
    let model = load_gateway_model(&state, caller.tenant_id, model_code).await?;
    validate_model_for_images(&model)?;

    let hold_minor = estimate_image_hold_minor(&payload, &model)?;
    let reservation = reserve_wallet_and_create_usage(
        &state,
        &caller,
        &model,
        &request_id.to_string(),
        endpoint,
        hold_minor,
        &payload,
        idempotency_key.as_deref(),
    )
    .await?;

    let provider_started = Instant::now();
    let provider_result =
        forward_openai_compatible_json(&state, &model, payload, "images/generations").await;
    metrics::record_ai_gateway_provider_duration(endpoint, provider_started.elapsed());

    match provider_result {
        Ok(mut provider_response) if provider_response.status.is_success() => {
            if let Err(error) = cache_image_assets(
                &state,
                caller.tenant_id,
                reservation.usage_id,
                &mut provider_response.body,
            )
            .await
            {
                release_usage(
                    &state,
                    &reservation,
                    provider_response.status.as_u16() as i32,
                    provider_response.provider_request_id.as_deref(),
                    Some(&provider_response.body),
                    "AI 图片缓存失败，释放预扣金额",
                )
                .await?;
                metrics::record_ai_gateway_asset_cache_failure();
                metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Error);

                return Err(error);
            }
            let charge_minor = calculate_image_charge_minor(&model, &provider_response.body)
                .unwrap_or(reservation.held_minor)
                .min(reservation.held_minor);
            capture_usage(
                &state,
                &reservation,
                &model,
                provider_response.status.as_u16() as i32,
                provider_response.provider_request_id.as_deref(),
                TokenUsage::default(),
                charge_minor,
                &provider_response.body,
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Success);
            metrics::record_ai_gateway_charged(endpoint, charge_minor);

            Ok(provider_json_response(
                provider_response.status,
                provider_response.body,
                reservation.usage_id,
            ))
        }
        Ok(provider_response) => {
            release_usage(
                &state,
                &reservation,
                provider_response.status.as_u16() as i32,
                provider_response.provider_request_id.as_deref(),
                Some(&provider_response.body),
                "AI 图片请求失败，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::ProviderError);

            Ok(provider_json_response(
                provider_response.status,
                provider_response.body,
                reservation.usage_id,
            ))
        }
        Err(error) => {
            release_usage(
                &state,
                &reservation,
                0,
                None,
                None,
                "AI 图片请求异常，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Error);

            Err(error)
        }
    }
}

pub async fn embeddings(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let caller = GatewayCaller::from_api_key(authenticate_api_key(&state, &headers).await?);
    embeddings_for_caller(state, request_id, headers, payload, caller).await
}

pub async fn client_embeddings(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Extension(client): Extension<ClientContext>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response, AppError> {
    let caller = GatewayCaller::from_client_context(&client)?;
    embeddings_for_caller(state, request_id, headers, payload, caller).await
}

async fn embeddings_for_caller(
    state: AppState,
    request_id: RequestId,
    headers: HeaderMap,
    payload: Value,
    caller: GatewayCaller,
) -> Result<Response, AppError> {
    check_gateway_rate_limit(&state, &caller).await?;
    let endpoint = "/v1/embeddings";
    let idempotency_key = idempotency_key(&headers)?;
    if let Some(response) =
        find_idempotent_response(&state, &caller, endpoint, idempotency_key.as_deref()).await?
    {
        return Ok(response);
    }
    let model_code = requested_model_code(&payload)?;
    let model = load_gateway_model(&state, caller.tenant_id, model_code).await?;
    validate_model_for_embeddings(&model)?;

    let hold_minor = estimate_embedding_hold_minor(&payload, &model)?;
    let reservation = reserve_wallet_and_create_usage(
        &state,
        &caller,
        &model,
        &request_id.to_string(),
        endpoint,
        hold_minor,
        &payload,
        idempotency_key.as_deref(),
    )
    .await?;

    let provider_started = Instant::now();
    let provider_result =
        forward_openai_compatible_json(&state, &model, payload, "embeddings").await;
    metrics::record_ai_gateway_provider_duration(endpoint, provider_started.elapsed());

    match provider_result {
        Ok(provider_response) if provider_response.status.is_success() => {
            let usage = token_usage_from_response(&provider_response.body);
            let charge_minor = calculate_embedding_charge_minor(&model, usage)
                .unwrap_or(reservation.held_minor)
                .min(reservation.held_minor);
            capture_usage(
                &state,
                &reservation,
                &model,
                provider_response.status.as_u16() as i32,
                provider_response.provider_request_id.as_deref(),
                usage,
                charge_minor,
                &provider_response.body,
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Success);
            metrics::record_ai_gateway_charged(endpoint, charge_minor);

            Ok(provider_json_response(
                provider_response.status,
                provider_response.body,
                reservation.usage_id,
            ))
        }
        Ok(provider_response) => {
            release_usage(
                &state,
                &reservation,
                provider_response.status.as_u16() as i32,
                provider_response.provider_request_id.as_deref(),
                Some(&provider_response.body),
                "AI Embeddings 请求失败，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::ProviderError);

            Ok(provider_json_response(
                provider_response.status,
                provider_response.body,
                reservation.usage_id,
            ))
        }
        Err(error) => {
            release_usage(
                &state,
                &reservation,
                0,
                None,
                None,
                "AI Embeddings 请求异常，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Error);

            Err(error)
        }
    }
}

pub async fn list_models(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let endpoint = "/v1/models";
    let caller = GatewayCaller::from_api_key(authenticate_api_key(&state, &headers).await?);
    list_models_for_caller(state, endpoint, caller).await
}

pub async fn client_list_models(
    State(state): State<AppState>,
    Extension(client): Extension<ClientContext>,
) -> Result<Response, AppError> {
    let endpoint = "/v1/models";
    let caller = GatewayCaller::from_client_context(&client)?;
    list_models_for_caller(state, endpoint, caller).await
}

async fn list_models_for_caller(
    state: AppState,
    endpoint: &'static str,
    caller: GatewayCaller,
) -> Result<Response, AppError> {
    check_gateway_rate_limit(&state, &caller).await?;
    let models = sqlx::query_as::<_, GatewayModelSummary>(
        r#"
        select
          m.code,
          m.created_at
        from ai_models m
        join ai_providers p
          on p.id = m.provider_id
          and p.tenant_id = m.tenant_id
        where m.tenant_id = $1
          and m.enabled
          and p.enabled
        order by m.code
        "#,
    )
    .bind(caller.tenant_id)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)?;
    let data = models
        .into_iter()
        .map(|model| {
            json!({
                "id": model.code,
                "object": "model",
                "created": model.created_at.timestamp(),
                "owned_by": "entitlehub",
            })
        })
        .collect::<Vec<_>>();
    metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Success);

    Ok((
        StatusCode::OK,
        Json(json!({ "object": "list", "data": data })),
    )
        .into_response())
}

pub async fn get_asset(
    State(state): State<AppState>,
    Path(asset_id): Path<Uuid>,
) -> Result<Response, AppError> {
    let asset = sqlx::query_as::<_, AiAssetRecord>(
        r#"
        select
          storage_key,
          mime_type,
          file_size
        from ai_assets
        where id = $1
          and status = 'ready'
        "#,
    )
    .bind(asset_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai asset not found"))?;
    let stored = state.object_store.open(&asset.storage_key).await?;
    let mut reader = stored.reader;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| AppError::dependency(format!("ai asset read failed: {error}")))?;
    let content_type = asset
        .mime_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    metrics::record_ai_gateway_request("/api/ai/assets/{id}", AiGatewayRequestStatus::Success);

    HttpResponse::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(
            "content-length",
            asset.file_size.unwrap_or(stored.size as i64).to_string(),
        )
        .header("cache-control", "public, max-age=31536000, immutable")
        .body(Body::from(bytes))
        .map_err(|error| AppError::dependency(format!("ai asset response failed: {error}")))
}

async fn forward_openai_compatible_json(
    state: &AppState,
    model: &GatewayModel,
    payload: Value,
    provider_path: &str,
) -> Result<ProviderResponse, AppError> {
    let secret = decrypt_provider_secret(state, model)?;
    let headers = provider_headers(&secret, &model.provider_config)?;
    let timeout = provider_timeout(&model.provider_config)?;
    let url = format!(
        "{}/{}",
        model.provider_base_url.trim_end_matches('/'),
        provider_path.trim_start_matches('/')
    );
    let outbound_payload = provider_payload(payload, model)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .map_err(|error| AppError::dependency(format!("ai provider client failed: {error}")))?;
    let response = client
        .post(url)
        .headers(headers)
        .json(&outbound_payload)
        .send()
        .await
        .map_err(|error| AppError::dependency(format!("ai provider request failed: {error}")))?;

    let status = response.status();
    let provider_request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("x-provider-request-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let text = response
        .text()
        .await
        .map_err(|error| AppError::dependency(format!("ai provider response failed: {error}")))?;
    let body = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| {
        json!({
            "error": {
                "message": "provider returned non-json response",
                "type": "provider_error",
                "status": status.as_u16()
            }
        })
    });

    Ok(ProviderResponse {
        status,
        provider_request_id,
        body,
    })
}

struct ProviderResponse {
    status: reqwest::StatusCode,
    provider_request_id: Option<String>,
    body: Value,
}

#[derive(Debug, FromRow)]
struct IdempotentUsageRecord {
    id: Uuid,
    status: String,
    provider_status: Option<String>,
    provider_raw_response: Option<Value>,
}

#[derive(Debug, Clone, FromRow)]
struct GatewayModelSummary {
    code: String,
    created_at: DateTime<Utc>,
}

async fn find_idempotent_response(
    state: &AppState,
    caller: &GatewayCaller,
    endpoint: &str,
    idempotency_key: Option<&str>,
) -> Result<Option<Response>, AppError> {
    let Some(idempotency_key) = idempotency_key else {
        return Ok(None);
    };
    let record = sqlx::query_as::<_, IdempotentUsageRecord>(
        r#"
        select
          id,
          status,
          provider_status,
          provider_raw_response
        from ai_usage_records
        where tenant_id = $1
          and customer_id = $2
          and api_key_id is not distinct from $3
          and endpoint = $4
          and idempotency_key = $5
        order by created_at desc
        limit 1
        "#,
    )
    .bind(caller.tenant_id)
    .bind(caller.customer_id)
    .bind(caller.api_key_id)
    .bind(endpoint)
    .bind(idempotency_key)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?;

    let Some(record) = record else {
        return Ok(None);
    };
    if matches!(record.status.as_str(), "pending" | "running") {
        return Err(AppError::conflict("ai idempotent request is still running"));
    }
    let Some(body) = record.provider_raw_response else {
        return Err(AppError::conflict(
            "ai idempotent request completed without reusable provider response",
        ));
    };
    let status = record
        .provider_status
        .as_deref()
        .and_then(|value| value.parse::<u16>().ok())
        .and_then(|value| StatusCode::from_u16(value).ok())
        .unwrap_or(StatusCode::BAD_GATEWAY);

    metrics::record_ai_gateway_request(endpoint, AiGatewayRequestStatus::Replay);
    metrics::record_ai_gateway_idempotency_replay(endpoint);

    Ok(Some(gateway_json_response(status, body, record.id)))
}

async fn load_gateway_model(
    state: &AppState,
    tenant_id: Uuid,
    model_code: &str,
) -> Result<GatewayModel, AppError> {
    sqlx::query_as::<_, GatewayModel>(
        r#"
        select
          m.id,
          p.id as provider_id,
          p.kind as provider_kind,
          p.name as provider_name,
          p.base_url as provider_base_url,
          p.config_json as provider_config,
          p.secret_encrypted as provider_secret_encrypted,
          m.code,
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
          m.daily_spend_limit_minor,
          m.pricing_config_json as pricing_config
        from ai_models m
        join ai_providers p
          on p.id = m.provider_id
          and p.tenant_id = m.tenant_id
        where m.tenant_id = $1
          and lower(m.code) = lower($2)
          and m.enabled
          and p.enabled
        "#,
    )
    .bind(tenant_id)
    .bind(model_code)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai model not found or disabled"))
}

async fn reserve_wallet_and_create_usage(
    state: &AppState,
    caller: &GatewayCaller,
    model: &GatewayModel,
    request_id: &str,
    endpoint: &str,
    hold_minor: i64,
    request_payload: &Value,
    idempotency_key: Option<&str>,
) -> Result<BillingReservation, AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_wallet_exists(&mut transaction, caller.tenant_id, caller.customer_id).await?;
    let wallet =
        find_wallet_for_update(&mut transaction, caller.tenant_id, caller.customer_id).await?;
    ensure_wallet_ai_enabled(&wallet)?;
    if wallet.currency != model.currency {
        return Err(AppError::business_rule_failed(
            "ai wallet currency does not match model currency",
        ));
    }
    if wallet.balance_minor - wallet.held_minor < hold_minor {
        return Err(AppError::business_rule_failed(
            "ai wallet available balance is insufficient",
        ));
    }
    ensure_daily_spend_limits(&mut transaction, caller, model, &wallet, hold_minor).await?;

    let usage_id = Uuid::new_v4();
    let updated_wallet = if hold_minor > 0 {
        update_wallet_hold(&mut transaction, caller.tenant_id, wallet.id, hold_minor).await?
    } else {
        wallet.clone()
    };
    insert_usage_record(
        &mut transaction,
        usage_id,
        caller,
        model,
        wallet.id,
        request_id,
        endpoint,
        hold_minor,
        request_payload,
        idempotency_key,
    )
    .await?;
    if hold_minor > 0 {
        insert_wallet_ledger_entry(
            &mut transaction,
            caller.tenant_id,
            &updated_wallet,
            "hold",
            hold_minor,
            "AI 请求预扣",
            usage_id,
            json!({
                "source": caller.source,
                "api_key_id": caller.api_key_id,
                "model": model.code,
            }),
        )
        .await?;
    }
    transaction.commit().await.map_err(map_db_error)?;

    Ok(BillingReservation {
        usage_id,
        wallet_id: wallet.id,
        held_minor: hold_minor,
    })
}

async fn ensure_daily_spend_limits(
    transaction: &mut Transaction<'_, Postgres>,
    caller: &GatewayCaller,
    model: &GatewayModel,
    wallet: &WalletRecord,
    hold_minor: i64,
) -> Result<(), AppError> {
    if hold_minor <= 0 {
        return Ok(());
    }

    if let Some(limit) = wallet.daily_spend_limit_minor {
        let used =
            daily_customer_spend_minor(transaction, caller.tenant_id, caller.customer_id).await?;
        ensure_daily_limit_available("customer ai daily spend limit", limit, used, hold_minor)?;
    }
    if let (Some(api_key_id), Some(limit)) =
        (caller.api_key_id, caller.api_key_daily_spend_limit_minor)
    {
        let used = daily_api_key_spend_minor(transaction, caller.tenant_id, api_key_id).await?;
        ensure_daily_limit_available("ai api key daily spend limit", limit, used, hold_minor)?;
    }
    if let Some(limit) = model.daily_spend_limit_minor {
        let used = daily_model_spend_minor(transaction, caller.tenant_id, model.id).await?;
        ensure_daily_limit_available("ai model daily spend limit", limit, used, hold_minor)?;
    }

    Ok(())
}

fn ensure_daily_limit_available(
    label: &str,
    limit_minor: i64,
    used_minor: i64,
    hold_minor: i64,
) -> Result<(), AppError> {
    if used_minor.saturating_add(hold_minor) > limit_minor {
        return Err(AppError::business_rule_failed(format!("{label} exceeded")));
    }

    Ok(())
}

fn ensure_wallet_ai_enabled(wallet: &WalletRecord) -> Result<(), AppError> {
    if !wallet.ai_enabled {
        return Err(AppError::business_rule_failed(
            "customer ai access is frozen",
        ));
    }

    Ok(())
}

async fn daily_customer_spend_minor(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<i64, AppError> {
    daily_spend_minor(transaction, tenant_id, Some(customer_id), None, None).await
}

async fn daily_api_key_spend_minor(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    api_key_id: Uuid,
) -> Result<i64, AppError> {
    daily_spend_minor(transaction, tenant_id, None, Some(api_key_id), None).await
}

async fn daily_model_spend_minor(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    model_id: Uuid,
) -> Result<i64, AppError> {
    daily_spend_minor(transaction, tenant_id, None, None, Some(model_id)).await
}

async fn daily_spend_minor(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Option<Uuid>,
    api_key_id: Option<Uuid>,
    model_id: Option<Uuid>,
) -> Result<i64, AppError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        select coalesce(sum(
          case
            when status = 'succeeded' then charged_minor
            when status in ('pending', 'running') then coalesce((price_snapshot_json->>'held_minor')::bigint, 0)
            else 0
          end
        ), 0)
        from ai_usage_records
        where tenant_id = $1
          and created_at >= date_trunc('day', now())
          and ($2::uuid is null or customer_id = $2)
          and ($3::uuid is null or api_key_id = $3)
          and ($4::uuid is null or model_id = $4)
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(api_key_id)
    .bind(model_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn capture_usage(
    state: &AppState,
    reservation: &BillingReservation,
    model: &GatewayModel,
    provider_status: i32,
    provider_request_id: Option<&str>,
    usage: TokenUsage,
    charge_minor: i64,
    provider_body: &Value,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let wallet = find_wallet_by_id_for_update(&mut transaction, reservation.wallet_id).await?;
    let updated_wallet = update_wallet_capture(
        &mut transaction,
        wallet.id,
        reservation.held_minor,
        charge_minor,
    )
    .await?;
    if charge_minor > 0 {
        insert_wallet_ledger_entry(
            &mut transaction,
            wallet.tenant_id,
            &updated_wallet,
            "capture",
            -charge_minor,
            "AI 请求成功结算",
            reservation.usage_id,
            json!({
                "model": model.code,
                "held_minor": reservation.held_minor,
                "released_minor": reservation.held_minor - charge_minor,
            }),
        )
        .await?;
    } else if reservation.held_minor > 0 {
        insert_wallet_ledger_entry(
            &mut transaction,
            wallet.tenant_id,
            &updated_wallet,
            "release",
            -reservation.held_minor,
            "AI 请求成功但无需扣费，释放预扣金额",
            reservation.usage_id,
            json!({
                "model": model.code,
            }),
        )
        .await?;
    }
    update_usage_succeeded(
        &mut transaction,
        reservation.usage_id,
        provider_status,
        provider_request_id,
        usage,
        charge_minor,
        provider_body,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)
}

async fn release_usage(
    state: &AppState,
    reservation: &BillingReservation,
    provider_status: i32,
    provider_request_id: Option<&str>,
    provider_body: Option<&Value>,
    reason: &str,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let wallet = find_wallet_by_id_for_update(&mut transaction, reservation.wallet_id).await?;
    let updated_wallet = if reservation.held_minor > 0 {
        update_wallet_hold(
            &mut transaction,
            wallet.tenant_id,
            wallet.id,
            -reservation.held_minor,
        )
        .await?
    } else {
        wallet.clone()
    };
    if reservation.held_minor > 0 {
        insert_wallet_ledger_entry(
            &mut transaction,
            wallet.tenant_id,
            &updated_wallet,
            "release",
            -reservation.held_minor,
            reason,
            reservation.usage_id,
            json!({}),
        )
        .await?;
    }
    update_usage_failed(
        &mut transaction,
        reservation.usage_id,
        provider_status,
        provider_request_id,
        reservation.held_minor,
        provider_body,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)
}

async fn check_gateway_rate_limit(
    state: &AppState,
    caller: &GatewayCaller,
) -> Result<(), AppError> {
    rate_limit::check_fixed_window(
        state,
        rate_limit::ai_gateway_key(&caller.rate_limit_key),
        state.config.security.ai_gateway_rate_limit_max,
        state.config.security.ai_gateway_rate_limit_window_seconds,
        AppError::rate_limited,
    )
    .await
}

async fn ensure_wallet_exists(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into ai_wallets (
          tenant_id,
          customer_id
        )
        values ($1, $2)
        on conflict (tenant_id, customer_id) do nothing
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn find_wallet_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<WalletRecord, AppError> {
    sqlx::query_as::<_, WalletRecord>(
        r#"
        select
          id,
          tenant_id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor
        from ai_wallets
        where tenant_id = $1
          and customer_id = $2
        for update
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn find_wallet_by_id_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    wallet_id: Uuid,
) -> Result<WalletRecord, AppError> {
    sqlx::query_as::<_, WalletRecord>(
        r#"
        select
          id,
          tenant_id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor
        from ai_wallets
        where id = $1
        for update
        "#,
    )
    .bind(wallet_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_wallet_hold(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    wallet_id: Uuid,
    delta_minor: i64,
) -> Result<WalletRecord, AppError> {
    sqlx::query_as::<_, WalletRecord>(
        r#"
        update ai_wallets
        set held_minor = held_minor + $3,
            updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          tenant_id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor
        "#,
    )
    .bind(tenant_id)
    .bind(wallet_id)
    .bind(delta_minor)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_wallet_capture(
    transaction: &mut Transaction<'_, Postgres>,
    wallet_id: Uuid,
    held_minor: i64,
    charge_minor: i64,
) -> Result<WalletRecord, AppError> {
    sqlx::query_as::<_, WalletRecord>(
        r#"
        update ai_wallets
        set balance_minor = balance_minor - $2,
            held_minor = held_minor - $3,
            updated_at = now()
        where id = $1
        returning
          id,
          tenant_id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor
        "#,
    )
    .bind(wallet_id)
    .bind(charge_minor)
    .bind(held_minor)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn insert_usage_record(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    caller: &GatewayCaller,
    model: &GatewayModel,
    wallet_id: Uuid,
    request_id: &str,
    endpoint: &str,
    held_minor: i64,
    request_payload: &Value,
    idempotency_key: Option<&str>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into ai_usage_records (
          id,
          tenant_id,
          wallet_id,
          customer_id,
          api_key_id,
          provider_id,
          model_id,
          request_id,
          idempotency_key,
          endpoint,
          status,
          charged_minor,
          price_snapshot_json,
          metadata_json
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'running', 0, $11, $12)
        "#,
    )
    .bind(usage_id)
    .bind(caller.tenant_id)
    .bind(wallet_id)
    .bind(caller.customer_id)
    .bind(caller.api_key_id)
    .bind(model.provider_id)
    .bind(model.id)
    .bind(request_id)
    .bind(idempotency_key)
    .bind(endpoint)
    .bind(price_snapshot(model, held_minor))
    .bind(json!({
        "source": caller.source,
        "api_key_id": caller.api_key_id,
        "request_model": requested_model_code(request_payload).unwrap_or(&model.code),
        "provider_name": model.provider_name,
    }))
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn insert_wallet_ledger_entry(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    wallet: &WalletRecord,
    entry_type: &str,
    amount_minor: i64,
    reason: &str,
    usage_id: Uuid,
    metadata: Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into ai_wallet_ledger_entries (
          tenant_id,
          wallet_id,
          customer_id,
          entry_type,
          amount_minor,
          balance_after_minor,
          held_after_minor,
          reason,
          reference_type,
          reference_id,
          metadata_json
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, 'ai_usage', $9, $10)
        "#,
    )
    .bind(tenant_id)
    .bind(wallet.id)
    .bind(wallet.customer_id)
    .bind(entry_type)
    .bind(amount_minor)
    .bind(wallet.balance_minor)
    .bind(wallet.held_minor)
    .bind(reason)
    .bind(usage_id.to_string())
    .bind(metadata)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn update_usage_succeeded(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    provider_status: i32,
    provider_request_id: Option<&str>,
    usage: TokenUsage,
    charged_minor: i64,
    provider_body: &Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_usage_records
        set status = 'succeeded',
            provider_status = $2,
            provider_request_id = $3,
            prompt_tokens = $4,
            completion_tokens = $5,
            total_tokens = $6,
            charged_minor = $7,
            provider_raw_response = $8,
            completed_at = now()
        where id = $1
        "#,
    )
    .bind(usage_id)
    .bind(provider_status.to_string())
    .bind(provider_request_id)
    .bind(usage.prompt_tokens)
    .bind(usage.completion_tokens)
    .bind(usage.total_tokens)
    .bind(charged_minor)
    .bind(provider_body)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn update_usage_failed(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    provider_status: i32,
    provider_request_id: Option<&str>,
    refunded_minor: i64,
    provider_body: Option<&Value>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_usage_records
        set status = 'failed',
            provider_status = $2,
            provider_request_id = $3,
            refunded_minor = $4,
            provider_raw_response = $5,
            completed_at = now()
        where id = $1
        "#,
    )
    .bind(usage_id)
    .bind(provider_status.to_string())
    .bind(provider_request_id)
    .bind(refunded_minor)
    .bind(provider_body)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

fn validate_model_for_chat(model: &GatewayModel) -> Result<(), AppError> {
    if model.provider_kind != "openai_compatible" {
        return Err(AppError::validation_failed(
            "ai provider is not openai compatible",
        ));
    }
    if !matches!(model.modality.as_str(), "text" | "multimodal") {
        return Err(AppError::validation_failed(
            "ai model does not support chat completions",
        ));
    }
    if model.billing_mode != "token" {
        return Err(AppError::validation_failed(
            "ai chat model billing mode must be token",
        ));
    }
    if model.provider_secret_encrypted.is_none() {
        return Err(AppError::validation_failed(
            "ai provider api key is not configured",
        ));
    }

    Ok(())
}

fn validate_model_for_images(model: &GatewayModel) -> Result<(), AppError> {
    if model.provider_kind != "openai_compatible" {
        return Err(AppError::validation_failed(
            "ai provider is not openai compatible",
        ));
    }
    if !matches!(model.modality.as_str(), "image" | "multimodal") {
        return Err(AppError::validation_failed(
            "ai model does not support image generations",
        ));
    }
    if model.billing_mode != "per_image" {
        return Err(AppError::validation_failed(
            "ai image model billing mode must be per_image",
        ));
    }
    if model.provider_secret_encrypted.is_none() {
        return Err(AppError::validation_failed(
            "ai provider api key is not configured",
        ));
    }

    Ok(())
}

fn validate_model_for_embeddings(model: &GatewayModel) -> Result<(), AppError> {
    if model.provider_kind != "openai_compatible" {
        return Err(AppError::validation_failed(
            "ai provider is not openai compatible",
        ));
    }
    if !matches!(model.modality.as_str(), "embedding" | "multimodal") {
        return Err(AppError::validation_failed(
            "ai model does not support embeddings",
        ));
    }
    if model.billing_mode != "token" {
        return Err(AppError::validation_failed(
            "ai embedding model billing mode must be token",
        ));
    }
    if model.provider_secret_encrypted.is_none() {
        return Err(AppError::validation_failed(
            "ai provider api key is not configured",
        ));
    }

    Ok(())
}

fn reject_streaming_request(payload: &Value) -> Result<(), AppError> {
    if payload.get("stream").and_then(Value::as_bool) == Some(true) {
        return Err(AppError::validation_failed(
            "streaming chat completions are not supported yet",
        ));
    }

    Ok(())
}

fn requested_model_code(payload: &Value) -> Result<&str, AppError> {
    payload
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::validation_failed("model is required"))
}

fn idempotency_key(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| AppError::validation_failed("idempotency-key header is invalid"))?
        .trim();
    if value.is_empty()
        || value.len() > MAX_IDEMPOTENCY_KEY_LEN
        || value.contains('\0')
        || value.chars().any(char::is_control)
    {
        return Err(AppError::validation_failed(
            "idempotency-key header is invalid",
        ));
    }

    Ok(Some(value.to_owned()))
}

fn provider_payload(mut payload: Value, model: &GatewayModel) -> Result<Value, AppError> {
    let provider_model = model.provider_model.as_deref().unwrap_or(&model.code);
    let object = payload
        .as_object_mut()
        .ok_or_else(|| AppError::validation_failed("ai gateway body must be an object"))?;
    object.insert("model".to_owned(), Value::String(provider_model.to_owned()));

    Ok(payload)
}

async fn cache_image_assets(
    state: &AppState,
    tenant_id: Uuid,
    usage_id: Uuid,
    body: &mut Value,
) -> Result<(), AppError> {
    let data = body
        .get_mut("data")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppError::dependency("ai image response missing data array"))?;
    let mut cached_count = 0;

    for item in data {
        let Some(object) = item.as_object_mut() else {
            continue;
        };
        if let Some(provider_url) = object.get("url").and_then(Value::as_str) {
            let asset = download_provider_asset(provider_url).await?;
            let public_url = store_ai_asset(
                state,
                tenant_id,
                usage_id,
                "image",
                Some(provider_url.to_owned()),
                asset.mime_type,
                asset.bytes,
            )
            .await?;
            object.insert("url".to_owned(), Value::String(public_url));
            cached_count += 1;
        } else if let Some(b64_json) = object.get("b64_json").and_then(Value::as_str) {
            let asset = decode_base64_image_asset(b64_json)?;
            let public_url = store_ai_asset(
                state,
                tenant_id,
                usage_id,
                "image",
                None,
                asset.mime_type,
                asset.bytes,
            )
            .await?;
            object.insert("url".to_owned(), Value::String(public_url));
            object.remove("b64_json");
            cached_count += 1;
        }
    }

    if cached_count == 0 {
        return Err(AppError::dependency(
            "ai image response did not include cacheable image data",
        ));
    }

    Ok(())
}

struct DecodedAsset {
    bytes: Vec<u8>,
    mime_type: String,
}

async fn download_provider_asset(provider_url: &str) -> Result<DecodedAsset, AppError> {
    let url = reqwest::Url::parse(provider_url)
        .map_err(|_| AppError::dependency("ai provider asset url is invalid"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(AppError::dependency(
            "ai provider asset url must be http or https",
        ));
    }
    ensure_provider_asset_url_allowed(&url)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| AppError::dependency(format!("ai asset client failed: {error}")))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| AppError::dependency(format!("ai asset download failed: {error}")))?;
    if !response.status().is_success() {
        return Err(AppError::dependency(format!(
            "ai asset download failed: status {}",
            response.status()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_AI_ASSET_BYTES)
    {
        return Err(AppError::dependency("ai asset is too large"));
    }
    let mime_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_mime_type)
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(|error| AppError::dependency(format!("ai asset download failed: {error}")))?;
        if (bytes.len() as u64) + (chunk.len() as u64) > MAX_AI_ASSET_BYTES {
            return Err(AppError::dependency("ai asset is too large"));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(DecodedAsset { bytes, mime_type })
}

fn ensure_provider_asset_url_allowed(url: &reqwest::Url) -> Result<(), AppError> {
    let host = url
        .host_str()
        .ok_or_else(|| AppError::dependency("ai provider asset url host is invalid"))?;
    let normalized_host = host.trim_end_matches('.').to_ascii_lowercase();
    if normalized_host == "localhost" || normalized_host.ends_with(".localhost") {
        return Err(AppError::dependency(
            "ai provider asset url host is not allowed",
        ));
    }
    if let Ok(ip) = normalized_host.parse::<IpAddr>() {
        if is_disallowed_asset_ip(ip) {
            return Err(AppError::dependency(
                "ai provider asset url ip is not allowed",
            ));
        }
    }

    Ok(())
}

fn is_disallowed_asset_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || octets[0] == 100 && (64..=127).contains(&octets[1])
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

fn decode_base64_image_asset(value: &str) -> Result<DecodedAsset, AppError> {
    let (mime_type, b64) = if let Some((metadata, data)) = value.split_once(',') {
        if metadata.starts_with("data:") && metadata.contains(";base64") {
            (
                normalize_mime_type(
                    metadata
                        .trim_start_matches("data:")
                        .trim_end_matches(";base64"),
                )
                .unwrap_or_else(|| "image/png".to_owned()),
                data,
            )
        } else {
            ("image/png".to_owned(), value)
        }
    } else {
        ("image/png".to_owned(), value)
    };
    let bytes = BASE64_STANDARD
        .decode(b64)
        .map_err(|_| AppError::dependency("ai image b64_json is invalid"))?;
    if bytes.len() as u64 > MAX_AI_ASSET_BYTES {
        return Err(AppError::dependency("ai asset is too large"));
    }

    Ok(DecodedAsset { bytes, mime_type })
}

async fn store_ai_asset(
    state: &AppState,
    tenant_id: Uuid,
    usage_id: Uuid,
    asset_type: &str,
    provider_url: Option<String>,
    mime_type: String,
    bytes: Vec<u8>,
) -> Result<String, AppError> {
    let asset_id = Uuid::new_v4();
    let extension = extension_for_mime(&mime_type);
    let storage_key = format!("tenants/{tenant_id}/ai-assets/{asset_id}.{extension}");
    state.object_store.put_bytes(&storage_key, &bytes).await?;
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    let public_url = asset_public_url(state, asset_id);

    sqlx::query(
        r#"
        insert into ai_assets (
          id,
          tenant_id,
          usage_id,
          asset_type,
          status,
          provider_url,
          storage_key,
          public_url,
          mime_type,
          file_size,
          checksum_sha256
        )
        values ($1, $2, $3, $4, 'ready', $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(asset_id)
    .bind(tenant_id)
    .bind(usage_id)
    .bind(asset_type)
    .bind(provider_url)
    .bind(&storage_key)
    .bind(&public_url)
    .bind(mime_type)
    .bind(bytes.len() as i64)
    .bind(checksum)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(public_url)
}

fn asset_public_url(state: &AppState, asset_id: Uuid) -> String {
    match state.config.app.base_url.as_deref() {
        Some(base_url) => format!(
            "{}/api/ai/assets/{asset_id}",
            base_url.trim_end_matches('/')
        ),
        None => format!("/api/ai/assets/{asset_id}"),
    }
}

fn normalize_mime_type(value: &str) -> Option<String> {
    let value = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    if value.is_empty() || value.contains('\0') || !value.contains('/') {
        None
    } else {
        Some(value)
    }
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "bin",
    }
}

fn decrypt_provider_secret(state: &AppState, model: &GatewayModel) -> Result<Value, AppError> {
    let Some(encrypted_secret) = model.provider_secret_encrypted.as_deref() else {
        return Ok(json!({}));
    };
    let envelope: PrivateKeyEnvelope = serde_json::from_str(encrypted_secret).map_err(|error| {
        AppError::crypto(format!("ai provider secret envelope invalid: {error}"))
    })?;
    let plaintext = decrypt_bytes(&state.config.security.master_key, &envelope)?;

    serde_json::from_slice(&plaintext)
        .map_err(|error| AppError::crypto(format!("ai provider secret plaintext invalid: {error}")))
}

fn provider_headers(secret: &Value, config: &Value) -> Result<ReqwestHeaderMap, AppError> {
    let mut headers = ReqwestHeaderMap::new();
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        ReqwestHeaderValue::from_static("application/json"),
    );

    if let Some(api_key) = secret.get("api_key").and_then(Value::as_str) {
        let value = format!("Bearer {}", api_key.trim());
        headers.insert(AUTHORIZATION, reqwest_header_value(&value)?);
    } else if let Some(authorization) = secret.get("authorization").and_then(Value::as_str) {
        headers.insert(AUTHORIZATION, reqwest_header_value(authorization.trim())?);
    } else {
        return Err(AppError::validation_failed(
            "ai provider secret must include api_key or authorization",
        ));
    }

    append_configured_headers(&mut headers, config.get("headers"))?;
    append_configured_headers(&mut headers, secret.get("headers"))?;

    Ok(headers)
}

fn append_configured_headers(
    headers: &mut ReqwestHeaderMap,
    value: Option<&Value>,
) -> Result<(), AppError> {
    let Some(Value::Object(map)) = value else {
        return Ok(());
    };
    for (key, value) in map {
        let Some(value) = value.as_str() else {
            return Err(AppError::validation_failed(
                "ai provider header values must be strings",
            ));
        };
        let name = ReqwestHeaderName::from_bytes(key.as_bytes())
            .map_err(|_| AppError::validation_failed("ai provider header name is invalid"))?;
        if matches!(
            name.as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            return Err(AppError::validation_failed(
                "ai provider header is not allowed",
            ));
        }
        headers.insert(name, reqwest_header_value(value)?);
    }

    Ok(())
}

fn reqwest_header_value(value: &str) -> Result<ReqwestHeaderValue, AppError> {
    ReqwestHeaderValue::from_str(value)
        .map_err(|_| AppError::validation_failed("ai provider header value is invalid"))
}

fn provider_timeout(config: &Value) -> Result<u64, AppError> {
    let timeout =
        if let Some(timeout_seconds) = config.get("timeout_seconds").and_then(Value::as_u64) {
            timeout_seconds
        } else if let Some(timeout_ms) = config.get("timeout_ms").and_then(Value::as_u64) {
            timeout_ms.saturating_add(999) / 1000
        } else {
            DEFAULT_TIMEOUT_SECONDS
        };
    if timeout == 0 || timeout > 600 {
        return Err(AppError::validation_failed(
            "ai provider timeout is invalid",
        ));
    }

    Ok(timeout)
}

fn estimate_hold_minor(payload: &Value, model: &GatewayModel) -> Result<i64, AppError> {
    let prompt_tokens = estimate_prompt_tokens(payload)?;
    let completion_tokens = completion_token_budget(payload);
    let input_minor = token_price_minor(prompt_tokens, model.input_1k_price_minor)?;
    let output_minor = token_price_minor(completion_tokens, model.output_1k_price_minor)?;

    model
        .request_price_minor
        .checked_add(input_minor)
        .and_then(|value| value.checked_add(output_minor))
        .ok_or_else(|| AppError::validation_failed("ai estimated charge is too large"))
}

fn estimate_image_hold_minor(payload: &Value, model: &GatewayModel) -> Result<i64, AppError> {
    let count = image_count(payload)?;
    let image_minor = model
        .image_price_minor
        .checked_mul(count)
        .ok_or_else(|| AppError::validation_failed("ai estimated image charge is too large"))?;

    model
        .request_price_minor
        .checked_add(image_minor)
        .ok_or_else(|| AppError::validation_failed("ai estimated charge is too large"))
}

fn calculate_image_charge_minor(model: &GatewayModel, provider_body: &Value) -> Option<i64> {
    let count = image_result_count(provider_body)?;
    model
        .request_price_minor
        .checked_add(model.image_price_minor.checked_mul(count)?)
}

fn image_result_count(provider_body: &Value) -> Option<i64> {
    provider_body
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.len() as i64)
        .filter(|count| *count > 0)
}

fn estimate_embedding_hold_minor(payload: &Value, model: &GatewayModel) -> Result<i64, AppError> {
    let input_tokens = estimate_embedding_input_tokens(payload)?;
    let input_minor = token_price_minor(input_tokens, model.input_1k_price_minor)?;

    model
        .request_price_minor
        .checked_add(input_minor)
        .ok_or_else(|| AppError::validation_failed("ai estimated charge is too large"))
}

fn image_count(payload: &Value) -> Result<i64, AppError> {
    let Some(value) = payload.get("n") else {
        return Ok(1);
    };
    let Some(count) = value.as_i64() else {
        return Err(AppError::validation_failed(
            "image count n must be an integer",
        ));
    };
    if !(1..=MAX_IMAGE_COUNT).contains(&count) {
        return Err(AppError::validation_failed(format!(
            "image count n must be between 1 and {MAX_IMAGE_COUNT}"
        )));
    }

    Ok(count)
}

fn calculate_actual_charge_minor(model: &GatewayModel, usage: TokenUsage) -> Option<i64> {
    let prompt_tokens = usage.prompt_tokens?;
    let completion_tokens = usage.completion_tokens?;
    model
        .request_price_minor
        .checked_add(token_price_minor(prompt_tokens, model.input_1k_price_minor).ok()?)
        .and_then(|value| {
            value.checked_add(
                token_price_minor(completion_tokens, model.output_1k_price_minor).ok()?,
            )
        })
}

fn calculate_embedding_charge_minor(model: &GatewayModel, usage: TokenUsage) -> Option<i64> {
    let input_tokens = usage.prompt_tokens.or(usage.total_tokens)?;
    model
        .request_price_minor
        .checked_add(token_price_minor(input_tokens, model.input_1k_price_minor).ok()?)
}

fn estimate_prompt_tokens(payload: &Value) -> Result<i64, AppError> {
    let source = payload.get("messages").unwrap_or(payload);
    let bytes = serde_json::to_vec(source)
        .map_err(|error| AppError::validation_failed(format!("chat body invalid: {error}")))?;
    let estimate = ((bytes.len() as i64) / 4 + 1).clamp(1, MAX_PROMPT_TOKEN_ESTIMATE);

    Ok(estimate)
}

fn estimate_embedding_input_tokens(payload: &Value) -> Result<i64, AppError> {
    let input = payload
        .get("input")
        .filter(|value| !value.is_null())
        .ok_or_else(|| AppError::validation_failed("embedding input is required"))?;
    let bytes = serde_json::to_vec(input).map_err(|error| {
        AppError::validation_failed(format!("embedding input invalid: {error}"))
    })?;
    let estimate = ((bytes.len() as i64) / 4 + 1).clamp(1, MAX_PROMPT_TOKEN_ESTIMATE);

    Ok(estimate)
}

fn completion_token_budget(payload: &Value) -> i64 {
    payload
        .get("max_completion_tokens")
        .or_else(|| payload.get("max_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(DEFAULT_COMPLETION_TOKEN_BUDGET)
        .clamp(0, MAX_COMPLETION_TOKEN_BUDGET)
}

fn token_price_minor(tokens: i64, price_per_1k_minor: i64) -> Result<i64, AppError> {
    if tokens <= 0 || price_per_1k_minor <= 0 {
        return Ok(0);
    }
    let numerator = (tokens as i128)
        .checked_mul(price_per_1k_minor as i128)
        .and_then(|value| value.checked_add(999))
        .ok_or_else(|| AppError::validation_failed("ai token charge is too large"))?;

    Ok((numerator / 1000) as i64)
}

fn token_usage_from_response(body: &Value) -> TokenUsage {
    let usage = body.get("usage");
    TokenUsage {
        prompt_tokens: usage
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(Value::as_i64),
        completion_tokens: usage
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(Value::as_i64),
        total_tokens: usage
            .and_then(|usage| usage.get("total_tokens"))
            .and_then(Value::as_i64),
    }
}

fn price_snapshot(model: &GatewayModel, held_minor: i64) -> Value {
    json!({
        "currency": &model.currency,
        "billing_mode": &model.billing_mode,
        "input_1k_price_minor": model.input_1k_price_minor,
        "output_1k_price_minor": model.output_1k_price_minor,
        "request_price_minor": model.request_price_minor,
        "image_price_minor": model.image_price_minor,
        "second_price_minor": model.second_price_minor,
        "minute_price_minor": model.minute_price_minor,
        "pricing_config": &model.pricing_config,
        "held_minor": held_minor,
        "captured_at": Utc::now(),
    })
}

fn provider_json_response(status: reqwest::StatusCode, body: Value, usage_id: Uuid) -> Response {
    let status = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    gateway_json_response(status, body, usage_id)
}

fn gateway_json_response(status: StatusCode, body: Value, usage_id: Uuid) -> Response {
    let mut response = (status, Json(body)).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(value) = HeaderValue::from_str(&usage_id.to_string()) {
        response
            .headers_mut()
            .insert("x-entitlehub-usage-id", value);
    }

    response
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("ai gateway database error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use crate::modules::client_auth::session::ClientContext;

    use super::{
        calculate_actual_charge_minor, calculate_embedding_charge_minor,
        calculate_image_charge_minor, completion_token_budget, decode_base64_image_asset,
        ensure_daily_limit_available, ensure_provider_asset_url_allowed, ensure_wallet_ai_enabled,
        estimate_embedding_hold_minor, estimate_hold_minor, estimate_image_hold_minor,
        extension_for_mime, idempotency_key, image_count, normalize_mime_type, provider_payload,
        provider_timeout, token_price_minor, token_usage_from_response, GatewayCaller,
        GatewayModel, WalletRecord,
    };

    #[test]
    fn token_price_rounds_up_to_minor_units() {
        assert_eq!(token_price_minor(1, 1).expect("price"), 1);
        assert_eq!(token_price_minor(1000, 250).expect("price"), 250);
        assert_eq!(token_price_minor(1001, 250).expect("price"), 251);
    }

    #[test]
    fn hold_uses_request_prompt_and_completion_budget() {
        let model = fixture_model();
        let payload = json!({
            "model": "gpt-test",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 10
        });
        let hold = estimate_hold_minor(&payload, &model).expect("hold");

        assert!(hold >= 101);
        assert_eq!(completion_token_budget(&payload), 10);
    }

    #[test]
    fn actual_charge_uses_provider_usage() {
        let model = fixture_model();
        let body = json!({
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 2000,
                "total_tokens": 3000
            }
        });
        let usage = token_usage_from_response(&body);

        assert_eq!(
            calculate_actual_charge_minor(&model, usage).expect("charge"),
            100 + 200 + 600
        );
    }

    #[test]
    fn embedding_charge_uses_prompt_or_total_tokens() {
        let model = GatewayModel {
            modality: "embedding".to_owned(),
            input_1k_price_minor: 120,
            request_price_minor: 10,
            ..fixture_model()
        };
        let body = json!({
            "usage": {
                "prompt_tokens": 1001,
                "total_tokens": 1001
            }
        });
        let usage = token_usage_from_response(&body);

        assert_eq!(
            calculate_embedding_charge_minor(&model, usage).expect("charge"),
            131
        );
    }

    #[test]
    fn provider_payload_rewrites_model_name() {
        let model = fixture_model();
        let payload =
            provider_payload(json!({"model": "public", "messages": []}), &model).expect("payload");

        assert_eq!(payload["model"], "provider-gpt-test");
    }

    #[test]
    fn image_hold_uses_request_price_and_image_count() {
        let model = GatewayModel {
            modality: "image".to_owned(),
            billing_mode: "per_image".to_owned(),
            image_price_minor: 250,
            ..fixture_model()
        };
        let payload = json!({
            "model": "image-test",
            "prompt": "hello",
            "n": 3
        });

        assert_eq!(image_count(&payload).expect("count"), 3);
        assert_eq!(
            estimate_image_hold_minor(&payload, &model).expect("hold"),
            850
        );
    }

    #[test]
    fn image_charge_uses_provider_result_count() {
        let model = GatewayModel {
            modality: "image".to_owned(),
            billing_mode: "per_image".to_owned(),
            request_price_minor: 0,
            image_price_minor: 250,
            ..fixture_model()
        };
        let body = json!({
            "data": [
                {"url": "https://cdn.example.com/1.png"},
                {"url": "https://cdn.example.com/2.png"}
            ]
        });

        assert_eq!(
            calculate_image_charge_minor(&model, &body).expect("charge"),
            500
        );
        assert!(calculate_image_charge_minor(&model, &json!({})).is_none());
    }

    #[test]
    fn embedding_hold_uses_input_estimate() {
        let model = GatewayModel {
            modality: "embedding".to_owned(),
            input_1k_price_minor: 1000,
            request_price_minor: 10,
            ..fixture_model()
        };
        let payload = json!({
            "model": "embedding-test",
            "input": "hello"
        });

        assert!(estimate_embedding_hold_minor(&payload, &model).expect("hold") >= 11);
        assert!(
            estimate_embedding_hold_minor(&json!({"model": "embedding-test"}), &model).is_err()
        );
    }

    #[test]
    fn image_count_rejects_out_of_range_values() {
        assert!(image_count(&json!({"n": 0})).is_err());
        assert!(image_count(&json!({"n": 11})).is_err());
        assert!(image_count(&json!({"n": "2"})).is_err());
    }

    #[test]
    fn daily_limit_rejects_when_hold_would_exceed_limit() {
        assert!(ensure_daily_limit_available("limit", 100, 80, 20).is_ok());
        assert!(ensure_daily_limit_available("limit", 100, 81, 20).is_err());
    }

    #[test]
    fn frozen_wallet_rejects_ai_gateway_usage() {
        let mut wallet = fixture_wallet();
        assert!(ensure_wallet_ai_enabled(&wallet).is_ok());

        wallet.ai_enabled = false;
        assert!(ensure_wallet_ai_enabled(&wallet).is_err());
    }

    #[test]
    fn client_context_builds_session_gateway_caller_without_api_key() {
        let tenant_id = Uuid::new_v4();
        let customer_id = Uuid::new_v4();
        let device_id = Uuid::new_v4();
        let client = fixture_client_context(tenant_id, Some(customer_id), device_id);

        let caller = GatewayCaller::from_client_context(&client).expect("caller");

        assert_eq!(caller.tenant_id, tenant_id);
        assert_eq!(caller.customer_id, customer_id);
        assert_eq!(caller.api_key_id, None);
        assert_eq!(caller.source, "client_session");
        assert_eq!(
            caller.rate_limit_key,
            format!("client:{customer_id}:{device_id}")
        );
    }

    #[test]
    fn client_context_without_customer_cannot_use_gateway_billing() {
        let client = fixture_client_context(Uuid::new_v4(), None, Uuid::new_v4());

        assert!(GatewayCaller::from_client_context(&client).is_err());
    }

    #[test]
    fn base64_image_asset_decodes_data_url() {
        let asset = decode_base64_image_asset("data:image/webp;base64,aGVsbG8=").expect("asset");

        assert_eq!(asset.bytes, b"hello");
        assert_eq!(asset.mime_type, "image/webp");
    }

    #[test]
    fn mime_helpers_normalize_and_pick_extensions() {
        assert_eq!(
            normalize_mime_type(" Image/PNG; charset=utf-8 ").as_deref(),
            Some("image/png")
        );
        assert_eq!(extension_for_mime("image/png"), "png");
        assert_eq!(extension_for_mime("application/octet-stream"), "bin");
    }

    #[test]
    fn provider_asset_url_rejects_local_network_hosts() {
        let loopback = reqwest::Url::parse("http://127.0.0.1/image.png").expect("url");
        let private = reqwest::Url::parse("https://10.0.0.1/image.png").expect("url");
        let localhost = reqwest::Url::parse("https://localhost/image.png").expect("url");
        let public = reqwest::Url::parse("https://cdn.example.com/image.png").expect("url");

        assert!(ensure_provider_asset_url_allowed(&loopback).is_err());
        assert!(ensure_provider_asset_url_allowed(&private).is_err());
        assert!(ensure_provider_asset_url_allowed(&localhost).is_err());
        assert!(ensure_provider_asset_url_allowed(&public).is_ok());
    }

    #[test]
    fn provider_timeout_accepts_seconds_or_milliseconds() {
        assert_eq!(provider_timeout(&json!({})).expect("timeout"), 120);
        assert_eq!(
            provider_timeout(&json!({"timeout_seconds": 30})).expect("timeout"),
            30
        );
        assert_eq!(
            provider_timeout(&json!({"timeout_ms": 1500})).expect("timeout"),
            2
        );
        assert!(provider_timeout(&json!({"timeout_ms": 0})).is_err());
    }

    #[test]
    fn idempotency_key_is_trimmed_and_validated() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "idempotency-key",
            axum::http::HeaderValue::from_static(" request-1 "),
        );
        assert_eq!(
            idempotency_key(&headers).expect("key").as_deref(),
            Some("request-1")
        );

        let mut invalid = axum::http::HeaderMap::new();
        invalid.insert("idempotency-key", axum::http::HeaderValue::from_static(""));
        assert!(idempotency_key(&invalid).is_err());
    }

    fn fixture_model() -> GatewayModel {
        GatewayModel {
            id: Uuid::nil(),
            provider_id: Uuid::nil(),
            provider_kind: "openai_compatible".to_owned(),
            provider_name: "Provider".to_owned(),
            provider_base_url: "https://api.example.com/v1".to_owned(),
            provider_config: json!({}),
            provider_secret_encrypted: Some("secret".to_owned()),
            code: "gpt-test".to_owned(),
            modality: "text".to_owned(),
            provider_model: Some("provider-gpt-test".to_owned()),
            currency: "CNY".to_owned(),
            billing_mode: "token".to_owned(),
            input_1k_price_minor: 200,
            output_1k_price_minor: 300,
            request_price_minor: 100,
            image_price_minor: 0,
            second_price_minor: 0,
            minute_price_minor: 0,
            daily_spend_limit_minor: None,
            pricing_config: json!({}),
        }
    }

    fn fixture_client_context(
        tenant_id: Uuid,
        customer_id: Option<Uuid>,
        device_id: Uuid,
    ) -> ClientContext {
        ClientContext {
            session_id: Uuid::new_v4(),
            tenant_id,
            app_id: Uuid::new_v4(),
            customer_id,
            device_id,
            machine_id: "machine-1".to_owned(),
            auth_mode: "account".to_owned(),
            entitlement_id: Uuid::new_v4(),
            entitlement_kind: "subscription".to_owned(),
            entitlement_status: "active".to_owned(),
            features: json!({}),
            entitlement_expires_at: Some(Utc::now()),
        }
    }

    fn fixture_wallet() -> WalletRecord {
        WalletRecord {
            id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            customer_id: Uuid::new_v4(),
            currency: "CNY".to_owned(),
            balance_minor: 1000,
            held_minor: 0,
            ai_enabled: true,
            daily_spend_limit_minor: None,
        }
    }
}
