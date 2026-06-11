use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use reqwest::header::{
    HeaderMap as ReqwestHeaderMap, HeaderName as ReqwestHeaderName,
    HeaderValue as ReqwestHeaderValue, AUTHORIZATION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, PrivateKeyEnvelope},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    metrics,
    modules::{
        auth::session::AdminContext,
        server_api::{
            ai_invoke_scope, authenticate_server_key, customer_id_from_headers,
            ensure_server_customer_subscription,
        },
    },
    rate_limit,
    state::AppState,
};

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const DEFAULT_POLL_INTERVAL_SECONDS: i64 = 15;
const DEFAULT_MAX_ATTEMPTS: i32 = 240;
const MAX_IMAGE_COUNT: i64 = 10;
const MAX_VIDEO_SECONDS: i64 = 3600;
const MAX_IDEMPOTENCY_KEY_LEN: usize = 200;
const MAX_IMAGE_ASSET_BYTES: u64 = 50 * 1024 * 1024;
const MAX_VIDEO_ASSET_BYTES: u64 = 512 * 1024 * 1024;
const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiGenerationJob {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub customer_email: Option<String>,
    pub customer_name: Option<String>,
    pub provider_name: Option<String>,
    pub model_code: Option<String>,
    pub usage_id: Option<Uuid>,
    pub request_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub job_type: String,
    pub status: String,
    pub provider_status: Option<String>,
    pub provider_job_id: Option<String>,
    pub provider_request_id: Option<String>,
    pub result: Option<Value>,
    pub asset_urls: Value,
    pub charge_mode: String,
    pub quantity: i64,
    pub held_minor: i64,
    pub charged_minor: i64,
    pub refunded_minor: i64,
    pub currency: String,
    pub failure_reason: Option<String>,
    pub attempts: i32,
    pub next_poll_at: Option<DateTime<Utc>>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AiGenerationJobResponse {
    pub job: AiGenerationJob,
}

#[derive(Debug, Serialize)]
pub struct AiGenerationJobListResponse {
    pub items: Vec<AiGenerationJob>,
    pub meta: ListMeta,
}

#[derive(Debug, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Deserialize)]
pub struct AiGenerationJobListQuery {
    pub status: Option<String>,
    pub job_type: Option<String>,
    pub customer_id: Option<Uuid>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct JobModel {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChargeSettlement {
    requested_minor: i64,
    captured_minor: i64,
    released_minor: i64,
    additional_minor: i64,
    shortfall_minor: i64,
}

#[derive(Debug, Clone, FromRow)]
struct JobWorkerRecord {
    id: Uuid,
    tenant_id: Uuid,
    usage_id: Option<Uuid>,
    provider_id: Option<Uuid>,
    job_type: String,
    provider_job_id: Option<String>,
    attempts: i32,
    held_minor: i64,
    charge_mode: String,
}

#[derive(Debug, Clone, FromRow)]
struct JobProviderRecord {
    kind: String,
    base_url: String,
    config: Value,
    secret_encrypted: Option<String>,
}

#[derive(Debug, Clone)]
struct ProviderSubmitResult {
    provider_job_id: String,
    provider_request_id: Option<String>,
    status: Option<String>,
    body: Value,
}

#[derive(Debug, Clone)]
struct ProviderPollResult {
    status: ProviderJobStatus,
    provider_status: Option<String>,
    body: Value,
    asset_urls: Vec<String>,
    failure_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderJobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone)]
struct DecodedAsset {
    bytes: Vec<u8>,
    mime_type: String,
}

pub async fn server_create_image_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    create_server_generation_job(state, request_id, headers, payload, "image").await
}

pub async fn server_create_video_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    create_server_generation_job(state, request_id, headers, payload, "video").await
}

pub async fn server_get_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(job_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    let server_key = authenticate_server_key(&state, &headers, ai_invoke_scope()).await?;
    let customer_id = customer_id_from_headers(&headers)?;
    ensure_server_customer_subscription(&state, &server_key, customer_id).await?;
    let job = find_generation_job(&state, server_key.tenant_id, customer_id, job_id).await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job },
        request_id.to_string(),
    )))
}

pub async fn list_ai_generation_jobs(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiGenerationJobListQuery>,
) -> Result<Json<ApiResponse<AiGenerationJobListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:job:read")?;
    let status = query.status.as_deref().map(normalize_status).transpose()?;
    let job_type = query
        .job_type
        .as_deref()
        .map(normalize_job_type)
        .transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_generation_jobs(
        &state,
        admin.tenant_id,
        status.as_deref(),
        job_type.as_deref(),
        query.customer_id,
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobListResponse {
            items,
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

pub fn spawn_ai_generation_job_worker(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        run_ai_generation_job_worker(state).await;
    })
}

async fn run_ai_generation_job_worker(state: AppState) {
    let mut interval =
        tokio::time::interval(Duration::from_secs(DEFAULT_POLL_INTERVAL_SECONDS as u64));
    loop {
        interval.tick().await;
        if let Err(error) = process_generation_job_batch(&state).await {
            tracing::warn!(%error, "ai generation job worker tick failed");
        }
    }
}

async fn create_server_generation_job(
    state: AppState,
    request_id: RequestId,
    headers: HeaderMap,
    payload: Value,
    job_type: &'static str,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    let server_key = authenticate_server_key(&state, &headers, ai_invoke_scope()).await?;
    let customer_id = customer_id_from_headers(&headers)?;
    ensure_server_customer_subscription(&state, &server_key, customer_id).await?;
    check_generation_rate_limit(&state, server_key.server_key_id, customer_id).await?;
    let idempotency_key = idempotency_key(&headers)?;
    if let Some(job) = find_idempotent_job(
        &state,
        server_key.tenant_id,
        customer_id,
        server_key.server_key_id,
        job_type,
        idempotency_key.as_deref(),
    )
    .await?
    {
        return Ok(Json(ApiResponse::ok(
            AiGenerationJobResponse { job },
            request_id.to_string(),
        )));
    }

    let model_code = requested_model_code(&payload)?;
    let model = load_job_model(&state, server_key.tenant_id, model_code).await?;
    validate_generation_model(&model, job_type)?;
    let quantity = estimated_quantity(job_type, &payload, &model)?;
    let hold_minor = estimate_generation_hold_minor(job_type, quantity, &model)?;

    let reservation = reserve_wallet_and_create_usage(
        &state,
        server_key.tenant_id,
        customer_id,
        Some(server_key.server_key_id),
        &model,
        &request_id.to_string(),
        generation_endpoint(job_type),
        hold_minor,
        &payload,
        idempotency_key.as_deref(),
    )
    .await?;
    let job_id = Uuid::new_v4();
    let submit_started = Instant::now();
    let submit_result =
        submit_provider_generation_job(&state, &model, job_type, payload.clone()).await;
    metrics::record_ai_gateway_provider_duration(
        generation_endpoint(job_type),
        submit_started.elapsed(),
    );

    let job = match submit_result {
        Ok(submit) => {
            let job = insert_submitted_job(
                &state,
                InsertJobInput {
                    id: job_id,
                    tenant_id: server_key.tenant_id,
                    wallet_id: reservation.wallet_id,
                    customer_id,
                    provider_id: model.provider_id,
                    model_id: model.id,
                    usage_id: reservation.usage_id,
                    server_key_id: server_key.server_key_id,
                    request_id: request_id.to_string(),
                    idempotency_key,
                    job_type,
                    provider_status: submit.status,
                    provider_job_id: submit.provider_job_id,
                    provider_request_id: submit.provider_request_id,
                    provider_submit_response: submit.body,
                    request_payload: payload,
                    charge_mode: model.billing_mode,
                    quantity,
                    held_minor: hold_minor,
                    currency: model.currency,
                },
            )
            .await?;
            metrics::record_ai_gateway_request(
                generation_endpoint(job_type),
                metrics::AiGatewayRequestStatus::Success,
            );
            job
        }
        Err(error) => {
            release_usage(
                &state,
                &reservation,
                0,
                None,
                None,
                "AI 生成任务提交失败，释放预扣金额",
            )
            .await?;
            metrics::record_ai_gateway_request(
                generation_endpoint(job_type),
                metrics::AiGatewayRequestStatus::Error,
            );
            return Err(error);
        }
    };

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job },
        request_id.to_string(),
    )))
}

struct InsertJobInput {
    id: Uuid,
    tenant_id: Uuid,
    wallet_id: Uuid,
    customer_id: Uuid,
    provider_id: Uuid,
    model_id: Uuid,
    usage_id: Uuid,
    server_key_id: Uuid,
    request_id: String,
    idempotency_key: Option<String>,
    job_type: &'static str,
    provider_status: Option<String>,
    provider_job_id: String,
    provider_request_id: Option<String>,
    provider_submit_response: Value,
    request_payload: Value,
    charge_mode: String,
    quantity: i64,
    held_minor: i64,
    currency: String,
}

async fn process_generation_job_batch(state: &AppState) -> Result<(), String> {
    let jobs = claim_pollable_generation_jobs(state)
        .await
        .map_err(|error| format!("claim ai generation jobs failed: {error}"))?;
    for job in jobs {
        if let Err(error) = process_generation_job(state, job.clone()).await {
            if let Err(mark_error) = mark_job_poll_error(state, &job, &error).await {
                tracing::warn!(%mark_error, job_id = %job.id, "mark ai generation job poll error failed");
            }
            tracing::warn!(%error, job_id = %job.id, "ai generation job processing failed");
        }
    }

    Ok(())
}

async fn process_generation_job(state: &AppState, job: JobWorkerRecord) -> Result<(), String> {
    let provider = load_job_provider(state, job.provider_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(provider_job_id) = job.provider_job_id.as_deref() else {
        fail_job_and_release_usage(state, &job, "AI 生成任务缺少第三方任务 ID", None, None)
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    };
    let poll = poll_provider_generation_job(state, &provider, provider_job_id)
        .await
        .map_err(|error| error.to_string())?;
    match poll.status {
        ProviderJobStatus::Pending | ProviderJobStatus::Running => {
            mark_job_running(state, job.id, poll.provider_status, poll.body)
                .await
                .map_err(|error| error.to_string())?;
        }
        ProviderJobStatus::Succeeded => {
            mark_job_provider_succeeded(state, job.id, poll.provider_status.clone(), &poll.body)
                .await
                .map_err(|error| error.to_string())?;
            match cache_generation_assets(
                state,
                job.tenant_id,
                job.usage_id,
                &job.job_type,
                poll.asset_urls.clone(),
            )
            .await
            {
                Ok(asset_urls) => {
                    let charged_minor = charge_minor_from_job(&job);
                    capture_usage(
                        state,
                        job.usage_id,
                        charged_minor,
                        &poll.body,
                        provider_job_id,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                    mark_job_succeeded(
                        state,
                        job.id,
                        charged_minor,
                        asset_urls,
                        poll.provider_status,
                        poll.body,
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                    metrics::record_ai_gateway_charged(
                        generation_endpoint(&job.job_type),
                        charged_minor,
                    );
                }
                Err(error) => {
                    metrics::record_ai_gateway_asset_cache_failure();
                    mark_job_caching_failed(state, job.id, &error.to_string(), poll.body)
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        ProviderJobStatus::Failed => {
            let reason = poll
                .failure_reason
                .as_deref()
                .unwrap_or("AI 生成任务第三方返回失败");
            fail_job_and_release_usage(state, &job, reason, poll.provider_status, Some(poll.body))
                .await
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

async fn submit_provider_generation_job(
    state: &AppState,
    model: &JobModel,
    job_type: &str,
    payload: Value,
) -> Result<ProviderSubmitResult, AppError> {
    match model.provider_kind.as_str() {
        "wuyin_keji" => submit_wuyin_generation_job(state, model, job_type, payload).await,
        _ => Err(AppError::validation_failed(
            "ai async generation provider is not supported",
        )),
    }
}

async fn poll_provider_generation_job(
    state: &AppState,
    provider: &JobProviderRecord,
    provider_job_id: &str,
) -> Result<ProviderPollResult, AppError> {
    match provider.kind.as_str() {
        "wuyin_keji" => poll_wuyin_generation_job(state, provider, provider_job_id).await,
        _ => Err(AppError::validation_failed(
            "ai async generation provider is not supported",
        )),
    }
}

async fn submit_wuyin_generation_job(
    state: &AppState,
    model: &JobModel,
    job_type: &str,
    payload: Value,
) -> Result<ProviderSubmitResult, AppError> {
    let secret = decrypt_provider_secret(state, model.provider_secret_encrypted.as_deref())?;
    let headers = provider_headers(&secret, &model.provider_config)?;
    let timeout = provider_timeout(&model.provider_config)?;
    let path = provider_submit_path(model, job_type)?;
    let outbound_payload = wuyin_submit_payload(payload, model)?;
    let url = format!(
        "{}/{}",
        model.provider_base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
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
    let provider_request_id = provider_request_id_from_headers(response.headers());
    let body = response_json(response).await?;
    if !status.is_success() {
        return Err(AppError::dependency(format!(
            "ai provider submit failed: status {status}"
        )));
    }
    let provider_job_id = provider_job_id_from_body(&body)?;

    Ok(ProviderSubmitResult {
        provider_job_id,
        provider_request_id,
        status: provider_status_string(&body),
        body,
    })
}

async fn poll_wuyin_generation_job(
    state: &AppState,
    provider: &JobProviderRecord,
    provider_job_id: &str,
) -> Result<ProviderPollResult, AppError> {
    let secret = decrypt_provider_secret(state, provider.secret_encrypted.as_deref())?;
    let headers = provider_headers(&secret, &provider.config)?;
    let timeout = provider_timeout(&provider.config)?;
    let detail_path = provider
        .config
        .get("detail_path")
        .or_else(|| provider.config.get("query_path"))
        .and_then(Value::as_str)
        .unwrap_or("/api/async/detail");
    let id_field = provider
        .config
        .get("detail_id_field")
        .or_else(|| provider.config.get("query_id_field"))
        .and_then(Value::as_str)
        .unwrap_or("id");
    let detail_method = provider
        .config
        .get("detail_method")
        .or_else(|| provider.config.get("query_method"))
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .trim()
        .to_ascii_uppercase();
    let url = format!(
        "{}/{}",
        provider.base_url.trim_end_matches('/'),
        detail_path.trim_start_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout))
        .build()
        .map_err(|error| AppError::dependency(format!("ai provider client failed: {error}")))?;
    let response = match detail_method.as_str() {
        "GET" => {
            client
                .get(url)
                .headers(headers)
                .query(&[(id_field, provider_job_id)])
                .send()
                .await
        }
        "POST" => {
            let mut body = Map::new();
            body.insert(
                id_field.to_owned(),
                Value::String(provider_job_id.to_owned()),
            );
            client
                .post(url)
                .headers(headers)
                .json(&Value::Object(body))
                .send()
                .await
        }
        _ => {
            return Err(AppError::validation_failed(
                "ai provider detail method must be GET or POST",
            ))
        }
    }
    .map_err(|error| AppError::dependency(format!("ai provider request failed: {error}")))?;
    let status = response.status();
    let body = response_json(response).await?;
    if !status.is_success() {
        return Err(AppError::dependency(format!(
            "ai provider detail failed: status {status}"
        )));
    }
    let provider_status = provider_status_string(&body);
    let status = wuyin_status_from_body(&body);
    let asset_urls = if status == ProviderJobStatus::Succeeded {
        collect_asset_urls(&body)
    } else {
        Vec::new()
    };
    let failure_reason = provider_failure_reason(&body);

    Ok(ProviderPollResult {
        status,
        provider_status,
        body,
        asset_urls,
        failure_reason,
    })
}

fn provider_submit_path(model: &JobModel, job_type: &str) -> Result<String, AppError> {
    if let Some(path) = model
        .pricing_config
        .get("submit_path")
        .and_then(Value::as_str)
    {
        return Ok(path.to_owned());
    }
    if let Some(path) = model
        .provider_config
        .get("submit_path")
        .and_then(Value::as_str)
    {
        return Ok(path.to_owned());
    }
    if let Some(paths) = model
        .provider_config
        .get("submit_paths")
        .and_then(Value::as_object)
    {
        if let Some(path) = paths.get(&model.code).and_then(Value::as_str) {
            return Ok(path.to_owned());
        }
        if let Some(provider_model) = model.provider_model.as_deref() {
            if let Some(path) = paths.get(provider_model).and_then(Value::as_str) {
                return Ok(path.to_owned());
            }
        }
    }

    let provider_model = model.provider_model.as_deref().unwrap_or(&model.code);
    let path = match provider_model {
        "google_omni" | "video_google_omni" => "/api/async/video_google_omni",
        "grok_imagine" | "video_grok_imagine" => "/api/async/video_grok_imagine",
        "image_gpt" | "gpt_image_2" | "gpt-image-2" | "GPT-Image-2" => "/api/async/image_gpt",
        "image_nanoBanana2" | "nanobanana2" | "NanoBanana2" => "/api/async/image_nanoBanana2",
        _ if job_type == "image" => "/api/async/image_gpt",
        _ if job_type == "video" => "/api/async/video_google_omni",
        _ => {
            return Err(AppError::validation_failed(
                "ai generation submit path is not configured",
            ))
        }
    };

    Ok(path.to_owned())
}

fn wuyin_submit_payload(mut payload: Value, model: &JobModel) -> Result<Value, AppError> {
    let object = payload
        .as_object_mut()
        .ok_or_else(|| AppError::validation_failed("ai generation body must be an object"))?;
    object.remove("model");
    if let Some(provider_model) = model.provider_model.as_deref() {
        object
            .entry("model".to_owned())
            .or_insert_with(|| Value::String(provider_model.to_owned()));
    }

    Ok(payload)
}

fn wuyin_status_from_body(body: &Value) -> ProviderJobStatus {
    let status = body
        .pointer("/data/status")
        .or_else(|| body.pointer("/status"))
        .or_else(|| body.pointer("/data/state"))
        .or_else(|| body.pointer("/state"));
    match status {
        Some(Value::Number(number)) => match number.as_i64() {
            Some(2) => ProviderJobStatus::Succeeded,
            Some(3) => ProviderJobStatus::Failed,
            Some(0) => ProviderJobStatus::Pending,
            _ => ProviderJobStatus::Running,
        },
        Some(Value::String(value)) => match value.trim().to_ascii_lowercase().as_str() {
            "2" | "success" | "succeeded" | "completed" | "done" => ProviderJobStatus::Succeeded,
            "3" | "failed" | "failure" | "error" => ProviderJobStatus::Failed,
            "0" | "pending" | "init" | "initializing" => ProviderJobStatus::Pending,
            _ => ProviderJobStatus::Running,
        },
        _ => ProviderJobStatus::Running,
    }
}

fn provider_job_id_from_body(body: &Value) -> Result<String, AppError> {
    for pointer in [
        "/data/id",
        "/data/task_id",
        "/data/job_id",
        "/id",
        "/task_id",
        "/job_id",
    ] {
        if let Some(value) = body.pointer(pointer).and_then(value_to_string) {
            let value = value.trim();
            if !value.is_empty() {
                return Ok(value.to_owned());
            }
        }
    }

    Err(AppError::dependency(
        "ai provider submit response missing task id",
    ))
}

fn provider_status_string(body: &Value) -> Option<String> {
    body.pointer("/data/status")
        .or_else(|| body.pointer("/status"))
        .or_else(|| body.pointer("/data/state"))
        .or_else(|| body.pointer("/state"))
        .and_then(value_to_string)
}

fn provider_failure_reason(body: &Value) -> Option<String> {
    for pointer in [
        "/data/error",
        "/data/message",
        "/error/message",
        "/message",
        "/msg",
    ] {
        if let Some(value) = body.pointer(pointer).and_then(value_to_string) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(truncate_error(value));
            }
        }
    }

    None
}

fn collect_asset_urls(value: &Value) -> Vec<String> {
    let mut urls = Vec::new();
    collect_asset_urls_in_value(value, &mut urls);
    urls.sort();
    urls.dedup();
    urls
}

fn collect_asset_urls_in_value(value: &Value, urls: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for key in [
                "url",
                "urls",
                "image_url",
                "image_urls",
                "video_url",
                "video_urls",
                "output_url",
                "download_url",
            ] {
                if let Some(value) = object.get(key) {
                    collect_string_urls(value, urls);
                }
            }
            for nested in object.values() {
                collect_asset_urls_in_value(nested, urls);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_asset_urls_in_value(item, urls);
            }
        }
        _ => {}
    }
}

fn collect_string_urls(value: &Value, urls: &mut Vec<String>) {
    match value {
        Value::String(value) => {
            if is_http_asset_url(value) {
                urls.push(value.to_owned());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_string_urls(item, urls);
            }
        }
        Value::Object(_) => collect_asset_urls_in_value(value, urls),
        _ => {}
    }
}

async fn cache_generation_assets(
    state: &AppState,
    tenant_id: Uuid,
    usage_id: Option<Uuid>,
    job_type: &str,
    provider_urls: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let usage_id =
        usage_id.ok_or_else(|| AppError::dependency("ai generation usage id missing"))?;
    if provider_urls.is_empty() {
        return Err(AppError::dependency(
            "ai generation response did not include cacheable asset urls",
        ));
    }
    let mut public_urls = Vec::new();
    let max_bytes = if job_type == "video" {
        MAX_VIDEO_ASSET_BYTES
    } else {
        MAX_IMAGE_ASSET_BYTES
    };
    for provider_url in provider_urls {
        let asset = download_provider_asset(&provider_url, max_bytes).await?;
        let public_url = store_ai_asset(
            state,
            tenant_id,
            usage_id,
            job_type,
            Some(provider_url),
            asset.mime_type,
            asset.bytes,
        )
        .await?;
        public_urls.push(public_url);
    }

    Ok(public_urls)
}

async fn download_provider_asset(
    provider_url: &str,
    max_bytes: u64,
) -> Result<DecodedAsset, AppError> {
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
        .is_some_and(|length| length > max_bytes)
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
        if (bytes.len() as u64) + (chunk.len() as u64) > max_bytes {
            return Err(AppError::dependency("ai asset is too large"));
        }
        bytes.extend_from_slice(&chunk);
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

async fn response_json(response: reqwest::Response) -> Result<Value, AppError> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| AppError::dependency(format!("ai provider response failed: {error}")))?;
    Ok(serde_json::from_str::<Value>(&text).unwrap_or_else(|_| {
        json!({
            "error": {
                "message": "provider returned non-json response",
                "type": "provider_error",
                "status": status.as_u16()
            }
        })
    }))
}

async fn load_job_model(
    state: &AppState,
    tenant_id: Uuid,
    model_code: &str,
) -> Result<JobModel, AppError> {
    sqlx::query_as::<_, JobModel>(
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

async fn load_job_provider(
    state: &AppState,
    provider_id: Option<Uuid>,
) -> Result<JobProviderRecord, AppError> {
    let provider_id =
        provider_id.ok_or_else(|| AppError::dependency("ai generation provider id missing"))?;
    sqlx::query_as::<_, JobProviderRecord>(
        r#"
        select
          kind,
          base_url,
          config_json as config,
          secret_encrypted
        from ai_providers
        where id = $1
          and enabled
        "#,
    )
    .bind(provider_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai provider not found or disabled"))
}

async fn reserve_wallet_and_create_usage(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    server_key_id: Option<Uuid>,
    model: &JobModel,
    request_id: &str,
    endpoint: &str,
    hold_minor: i64,
    request_payload: &Value,
    idempotency_key: Option<&str>,
) -> Result<BillingReservation, AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_wallet_exists(&mut transaction, tenant_id, customer_id).await?;
    let wallet = find_wallet_for_update(&mut transaction, tenant_id, customer_id).await?;
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
    ensure_daily_spend_limits(
        &mut transaction,
        tenant_id,
        customer_id,
        model,
        &wallet,
        hold_minor,
    )
    .await?;

    let usage_id = Uuid::new_v4();
    let updated_wallet = if hold_minor > 0 {
        update_wallet_hold(&mut transaction, tenant_id, wallet.id, hold_minor).await?
    } else {
        wallet.clone()
    };
    insert_usage_record(
        &mut transaction,
        usage_id,
        tenant_id,
        customer_id,
        server_key_id,
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
            tenant_id,
            &updated_wallet,
            "hold",
            hold_minor,
            "AI 生成任务预扣",
            usage_id,
            json!({
                "source": "server_key",
                "server_key_id": server_key_id,
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

async fn capture_usage(
    state: &AppState,
    usage_id: Option<Uuid>,
    charge_minor: i64,
    provider_body: &Value,
    provider_job_id: &str,
) -> Result<(), AppError> {
    let usage_id = usage_id.ok_or_else(|| AppError::dependency("ai usage id missing"))?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let usage = find_usage_for_update(&mut transaction, usage_id).await?;
    if usage.status == "succeeded" {
        transaction.commit().await.map_err(map_db_error)?;
        return Ok(());
    }
    let wallet = find_wallet_by_id_for_update(&mut transaction, usage.wallet_id).await?;
    let settlement = settle_charge(
        charge_minor,
        usage.held_minor,
        wallet.balance_minor,
        wallet.held_minor,
    );
    let updated_wallet = update_wallet_capture(
        &mut transaction,
        wallet.id,
        usage.held_minor,
        settlement.captured_minor,
    )
    .await?;
    if settlement.captured_minor > 0 {
        insert_wallet_ledger_entry(
            &mut transaction,
            wallet.tenant_id,
            &updated_wallet,
            "capture",
            -settlement.captured_minor,
            "AI 生成任务成功结算",
            usage_id,
            settlement_metadata(settlement),
        )
        .await?;
    } else if usage.held_minor > 0 {
        insert_wallet_ledger_entry(
            &mut transaction,
            wallet.tenant_id,
            &updated_wallet,
            "release",
            -usage.held_minor,
            "AI 生成任务成功但无需扣费，释放预扣金额",
            usage_id,
            json!({}),
        )
        .await?;
    }
    update_usage_succeeded(
        &mut transaction,
        usage_id,
        provider_job_id,
        settlement.captured_minor,
        settlement_metadata(settlement),
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

async fn fail_job_and_release_usage(
    state: &AppState,
    job: &JobWorkerRecord,
    reason: &str,
    provider_status: Option<String>,
    provider_body: Option<Value>,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let usage_id = job
        .usage_id
        .ok_or_else(|| AppError::dependency("ai generation usage id missing"))?;
    let usage = find_usage_for_update(&mut transaction, usage_id).await?;
    if usage.status != "failed" && usage.status != "succeeded" {
        let wallet = find_wallet_by_id_for_update(&mut transaction, usage.wallet_id).await?;
        let updated_wallet = if usage.held_minor > 0 {
            update_wallet_hold(
                &mut transaction,
                wallet.tenant_id,
                wallet.id,
                -usage.held_minor,
            )
            .await?
        } else {
            wallet.clone()
        };
        if usage.held_minor > 0 {
            insert_wallet_ledger_entry(
                &mut transaction,
                wallet.tenant_id,
                &updated_wallet,
                "release",
                -usage.held_minor,
                reason,
                usage_id,
                json!({}),
            )
            .await?;
        }
        update_usage_failed(
            &mut transaction,
            usage_id,
            provider_status
                .as_deref()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0),
            job.provider_job_id.as_deref(),
            usage.held_minor,
            provider_body.as_ref(),
        )
        .await?;
    }
    update_job_failed(
        &mut transaction,
        job.id,
        "provider_failed",
        provider_status,
        reason,
        provider_body.as_ref(),
        usage.held_minor,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)
}

#[derive(Debug, FromRow)]
struct UsageForUpdate {
    wallet_id: Uuid,
    status: String,
    held_minor: i64,
}

async fn find_usage_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
) -> Result<UsageForUpdate, AppError> {
    sqlx::query_as::<_, UsageForUpdate>(
        r#"
        select
          wallet_id,
          status,
          coalesce((price_snapshot_json->>'held_minor')::bigint, 0) as held_minor
        from ai_usage_records
        where id = $1
        for update
        "#,
    )
    .bind(usage_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
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

async fn insert_usage_record(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    tenant_id: Uuid,
    customer_id: Uuid,
    server_key_id: Option<Uuid>,
    model: &JobModel,
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
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'running', 0, $10, $11)
        "#,
    )
    .bind(usage_id)
    .bind(tenant_id)
    .bind(wallet_id)
    .bind(customer_id)
    .bind(model.provider_id)
    .bind(model.id)
    .bind(request_id)
    .bind(idempotency_key)
    .bind(endpoint)
    .bind(price_snapshot(model, held_minor))
    .bind(json!({
        "source": "server_key",
        "server_key_id": server_key_id,
        "request_model": requested_model_code(request_payload).unwrap_or(&model.code),
        "provider_name": model.provider_name,
    }))
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn update_usage_succeeded(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    provider_job_id: &str,
    charged_minor: i64,
    settlement_metadata: Value,
    provider_body: &Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_usage_records
        set status = 'succeeded',
            provider_status = '2',
            provider_request_id = $2,
            charged_minor = $3,
            metadata_json = metadata_json || $4::jsonb,
            provider_raw_response = $5,
            completed_at = now()
        where id = $1
        "#,
    )
    .bind(usage_id)
    .bind(provider_job_id)
    .bind(charged_minor)
    .bind(settlement_metadata)
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

async fn insert_submitted_job(
    state: &AppState,
    input: InsertJobInput,
) -> Result<AiGenerationJob, AppError> {
    sqlx::query(
        r#"
        insert into ai_generation_jobs (
          id,
          tenant_id,
          wallet_id,
          customer_id,
          provider_id,
          model_id,
          usage_id,
          server_key_id,
          request_id,
          idempotency_key,
          job_type,
          status,
          provider_status,
          provider_job_id,
          provider_request_id,
          provider_submit_response,
          request_payload,
          charge_mode,
          quantity,
          held_minor,
          currency,
          next_poll_at,
          submitted_at
        )
        values (
          $1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
          $11, 'submitted', $12, $13, $14, $15, $16, $17,
          $18, $19, $20, now() + interval '5 seconds', now()
        )
        "#,
    )
    .bind(input.id)
    .bind(input.tenant_id)
    .bind(input.wallet_id)
    .bind(input.customer_id)
    .bind(input.provider_id)
    .bind(input.model_id)
    .bind(input.usage_id)
    .bind(input.server_key_id)
    .bind(input.request_id)
    .bind(input.idempotency_key)
    .bind(input.job_type)
    .bind(input.provider_status)
    .bind(input.provider_job_id)
    .bind(input.provider_request_id)
    .bind(input.provider_submit_response)
    .bind(input.request_payload)
    .bind(input.charge_mode)
    .bind(input.quantity)
    .bind(input.held_minor)
    .bind(input.currency)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    find_generation_job_by_id(state, input.tenant_id, input.id).await
}

async fn claim_pollable_generation_jobs(
    state: &AppState,
) -> Result<Vec<JobWorkerRecord>, sqlx::Error> {
    sqlx::query_as::<_, JobWorkerRecord>(
        r#"
        update ai_generation_jobs
        set
          status = case
            when status in ('submitted', 'pending') then 'running'
            else status
          end,
          attempts = attempts + 1,
          next_poll_at = now() + interval '60 seconds',
          updated_at = now()
        where id in (
          select id
          from ai_generation_jobs
          where status in ('submitted', 'running', 'caching')
            and (next_poll_at is null or next_poll_at <= now())
          order by next_poll_at asc nulls first, created_at asc
          limit 20
          for update skip locked
        )
        returning
          id,
          tenant_id,
          usage_id,
          provider_id,
          job_type,
          provider_job_id,
          attempts,
          held_minor,
          charge_mode
        "#,
    )
    .fetch_all(&state.db)
    .await
}

async fn mark_job_running(
    state: &AppState,
    job_id: Uuid,
    provider_status: Option<String>,
    provider_body: Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'running',
            provider_status = $2,
            provider_result_response = $3,
            next_poll_at = now() + interval '15 seconds',
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job_id)
    .bind(provider_status)
    .bind(provider_body)
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn mark_job_provider_succeeded(
    state: &AppState,
    job_id: Uuid,
    provider_status: Option<String>,
    provider_body: &Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'caching',
            provider_status = $2,
            provider_result_response = $3,
            next_poll_at = now() + interval '30 seconds',
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job_id)
    .bind(provider_status)
    .bind(provider_body)
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn mark_job_succeeded(
    state: &AppState,
    job_id: Uuid,
    charged_minor: i64,
    asset_urls: Vec<String>,
    provider_status: Option<String>,
    provider_body: Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'succeeded',
            provider_status = $2,
            provider_result_response = $3,
            result_json = $4,
            asset_urls = $5,
            charged_minor = $6,
            next_poll_at = null,
            completed_at = now(),
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job_id)
    .bind(provider_status)
    .bind(&provider_body)
    .bind(json!({
        "provider": provider_body,
        "asset_urls": asset_urls,
    }))
    .bind(json!(asset_urls))
    .bind(charged_minor)
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn mark_job_caching_failed(
    state: &AppState,
    job_id: Uuid,
    error: &str,
    provider_body: Value,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'caching',
            provider_result_response = $2,
            failure_reason = $3,
            next_poll_at = now() + interval '60 seconds',
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job_id)
    .bind(provider_body)
    .bind(truncate_error(error))
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn update_job_failed(
    transaction: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
    status: &str,
    provider_status: Option<String>,
    reason: &str,
    provider_body: Option<&Value>,
    refunded_minor: i64,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = $2,
            provider_status = $3,
            provider_result_response = $4,
            failure_reason = $5,
            refunded_minor = $6,
            next_poll_at = null,
            completed_at = now(),
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job_id)
    .bind(status)
    .bind(provider_status)
    .bind(provider_body)
    .bind(truncate_error(reason))
    .bind(refunded_minor)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn mark_job_poll_error(
    state: &AppState,
    job: &JobWorkerRecord,
    error: &str,
) -> Result<(), AppError> {
    let status = if job.attempts >= DEFAULT_MAX_ATTEMPTS {
        "timeout_review"
    } else {
        "running"
    };
    let next_poll_sql = if status == "timeout_review" {
        "null"
    } else {
        "now() + interval '60 seconds'"
    };
    let sql = format!(
        r#"
        update ai_generation_jobs
        set status = $2,
            failure_reason = $3,
            next_poll_at = {next_poll_sql},
            updated_at = now()
        where id = $1
        "#
    );
    sqlx::query(&sql)
        .bind(job.id)
        .bind(status)
        .bind(truncate_error(error))
        .execute(&state.db)
        .await
        .map(|_| ())
        .map_err(map_db_error)
}

async fn find_generation_job(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    job_id: Uuid,
) -> Result<AiGenerationJob, AppError> {
    sqlx::query_as::<_, AiGenerationJob>(&format!(
        "{} where j.tenant_id = $1 and j.customer_id = $2 and j.id = $3",
        job_select_sql()
    ))
    .bind(tenant_id)
    .bind(customer_id)
    .bind(job_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai generation job not found"))
}

async fn find_generation_job_by_id(
    state: &AppState,
    tenant_id: Uuid,
    job_id: Uuid,
) -> Result<AiGenerationJob, AppError> {
    sqlx::query_as::<_, AiGenerationJob>(&format!(
        "{} where j.tenant_id = $1 and j.id = $2",
        job_select_sql()
    ))
    .bind(tenant_id)
    .bind(job_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai generation job not found"))
}

async fn find_idempotent_job(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    server_key_id: Uuid,
    job_type: &str,
    idempotency_key: Option<&str>,
) -> Result<Option<AiGenerationJob>, AppError> {
    let Some(idempotency_key) = idempotency_key else {
        return Ok(None);
    };
    sqlx::query_as::<_, AiGenerationJob>(&format!(
        "{} where j.tenant_id = $1 and j.customer_id = $2 and j.server_key_id = $3 and j.job_type = $4 and j.idempotency_key = $5",
        job_select_sql()
    ))
    .bind(tenant_id)
    .bind(customer_id)
    .bind(server_key_id)
    .bind(job_type)
    .bind(idempotency_key)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)
}

async fn list_generation_jobs(
    state: &AppState,
    tenant_id: Uuid,
    status: Option<&str>,
    job_type: Option<&str>,
    customer_id: Option<Uuid>,
    page: i64,
    page_size: i64,
) -> Result<Vec<AiGenerationJob>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, AiGenerationJob>(&format!(
        "{} where j.tenant_id = $1 and ($2::text is null or j.status = $2) and ($3::text is null or j.job_type = $3) and ($4::uuid is null or j.customer_id = $4) order by j.created_at desc, j.id desc limit $5 offset $6",
        job_select_sql()
    ))
    .bind(tenant_id)
    .bind(status)
    .bind(job_type)
    .bind(customer_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

fn job_select_sql() -> &'static str {
    r#"
    select
      j.id,
      j.customer_id,
      c.email as customer_email,
      c.name as customer_name,
      p.name as provider_name,
      m.code as model_code,
      j.usage_id,
      j.request_id,
      j.idempotency_key,
      j.job_type,
      j.status,
      j.provider_status,
      j.provider_job_id,
      j.provider_request_id,
      j.result_json as result,
      j.asset_urls,
      j.charge_mode,
      j.quantity,
      j.held_minor,
      j.charged_minor,
      j.refunded_minor,
      j.currency,
      j.failure_reason,
      j.attempts,
      j.next_poll_at,
      j.submitted_at,
      j.completed_at,
      j.created_at,
      j.updated_at
    from ai_generation_jobs j
    left join customers c
      on c.id = j.customer_id
      and c.tenant_id = j.tenant_id
    left join ai_providers p
      on p.id = j.provider_id
      and p.tenant_id = j.tenant_id
    left join ai_models m
      on m.id = j.model_id
      and m.tenant_id = j.tenant_id
    "#
}

async fn ensure_daily_spend_limits(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    model: &JobModel,
    wallet: &WalletRecord,
    hold_minor: i64,
) -> Result<(), AppError> {
    if hold_minor <= 0 {
        return Ok(());
    }
    if let Some(limit) = wallet.daily_spend_limit_minor {
        let used = daily_spend_minor(transaction, tenant_id, Some(customer_id), None).await?;
        ensure_daily_limit_available("customer ai daily spend limit", limit, used, hold_minor)?;
    }
    if let Some(limit) = model.daily_spend_limit_minor {
        let used = daily_spend_minor(transaction, tenant_id, None, Some(model.id)).await?;
        ensure_daily_limit_available("ai model daily spend limit", limit, used, hold_minor)?;
    }

    Ok(())
}

async fn daily_spend_minor(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Option<Uuid>,
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
          and ($3::uuid is null or model_id = $3)
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(model_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
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

fn validate_generation_model(model: &JobModel, job_type: &str) -> Result<(), AppError> {
    if model.provider_kind != "wuyin_keji" {
        return Err(AppError::validation_failed(
            "ai async generation model provider must be wuyin_keji",
        ));
    }
    match job_type {
        "image" => {
            if !matches!(model.modality.as_str(), "image" | "multimodal") {
                return Err(AppError::validation_failed(
                    "ai model does not support image jobs",
                ));
            }
            if model.billing_mode != "per_image" {
                return Err(AppError::validation_failed(
                    "ai image job billing mode must be per_image",
                ));
            }
        }
        "video" => {
            if !matches!(model.modality.as_str(), "video" | "multimodal") {
                return Err(AppError::validation_failed(
                    "ai model does not support video jobs",
                ));
            }
            if !matches!(
                model.billing_mode.as_str(),
                "video_per_second" | "video_per_request"
            ) {
                return Err(AppError::validation_failed(
                    "ai video job billing mode must be video_per_second or video_per_request",
                ));
            }
        }
        _ => {
            return Err(AppError::validation_failed(
                "ai generation job type invalid",
            ))
        }
    }
    if model.provider_secret_encrypted.is_none() {
        return Err(AppError::validation_failed(
            "ai provider api key is not configured",
        ));
    }

    Ok(())
}

fn estimated_quantity(job_type: &str, payload: &Value, model: &JobModel) -> Result<i64, AppError> {
    match job_type {
        "image" => image_count(payload),
        "video" if model.billing_mode == "video_per_second" => requested_video_seconds(payload),
        "video" => Ok(1),
        _ => Err(AppError::validation_failed(
            "ai generation job type invalid",
        )),
    }
}

fn estimate_generation_hold_minor(
    job_type: &str,
    quantity: i64,
    model: &JobModel,
) -> Result<i64, AppError> {
    match (job_type, model.billing_mode.as_str()) {
        ("image", "per_image") => model
            .request_price_minor
            .checked_add(
                model
                    .image_price_minor
                    .checked_mul(quantity)
                    .ok_or_else(|| {
                        AppError::validation_failed("ai estimated image charge is too large")
                    })?,
            )
            .ok_or_else(|| AppError::validation_failed("ai estimated charge is too large")),
        ("video", "video_per_second") => model
            .request_price_minor
            .checked_add(
                model
                    .second_price_minor
                    .checked_mul(quantity)
                    .ok_or_else(|| {
                        AppError::validation_failed("ai estimated video charge is too large")
                    })?,
            )
            .ok_or_else(|| AppError::validation_failed("ai estimated charge is too large")),
        ("video", "video_per_request") => Ok(model.request_price_minor),
        _ => Err(AppError::validation_failed(
            "ai generation billing mode is invalid",
        )),
    }
}

fn charge_minor_from_job(job: &JobWorkerRecord) -> i64 {
    match job.charge_mode.as_str() {
        "per_image" | "video_per_second" => job.held_minor,
        "video_per_request" => job.held_minor,
        _ => job.held_minor,
    }
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

fn requested_video_seconds(payload: &Value) -> Result<i64, AppError> {
    let seconds = optional_video_seconds(payload)?
        .or_else(|| {
            payload
                .get("video")
                .and_then(|video| optional_video_seconds(video).ok())
                .flatten()
        })
        .unwrap_or(8);
    if !(1..=MAX_VIDEO_SECONDS).contains(&seconds) {
        return Err(AppError::validation_failed(format!(
            "video duration must be between 1 and {MAX_VIDEO_SECONDS} seconds"
        )));
    }

    Ok(seconds)
}

fn optional_video_seconds(value: &Value) -> Result<Option<i64>, AppError> {
    for key in ["duration", "duration_seconds", "seconds"] {
        if let Some(raw) = value.get(key) {
            return value_to_positive_seconds(raw)
                .map(Some)
                .ok_or_else(|| AppError::validation_failed("video duration is invalid"));
        }
    }

    Ok(None)
}

fn value_to_positive_seconds(value: &Value) -> Option<i64> {
    if let Some(value) = value.as_i64() {
        return (value > 0).then_some(value);
    }
    if let Some(value) = value.as_f64() {
        return (value.is_finite() && value > 0.0).then_some(value.ceil() as i64);
    }
    value
        .as_str()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .map(|value| value.ceil() as i64)
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

fn generation_endpoint(job_type: &str) -> &'static str {
    match job_type {
        "image" => "/api/server/ai/v1/images/jobs",
        "video" => "/api/server/ai/v1/videos/jobs",
        _ => "/api/server/ai/v1/jobs",
    }
}

fn settle_charge(
    requested_minor: i64,
    reservation_held_minor: i64,
    wallet_balance_minor: i64,
    wallet_held_minor: i64,
) -> ChargeSettlement {
    let requested_minor = requested_minor.max(0);
    let reservation_held_minor = reservation_held_minor.max(0);
    let available_minor = wallet_balance_minor
        .saturating_sub(wallet_held_minor)
        .max(0);
    let max_capturable_minor = reservation_held_minor.saturating_add(available_minor);
    let captured_minor = requested_minor.min(max_capturable_minor);

    ChargeSettlement {
        requested_minor,
        captured_minor,
        released_minor: (reservation_held_minor - captured_minor).max(0),
        additional_minor: (captured_minor - reservation_held_minor).max(0),
        shortfall_minor: (requested_minor - captured_minor).max(0),
    }
}

fn settlement_metadata(settlement: ChargeSettlement) -> Value {
    json!({
        "billing_settlement": {
            "requested_minor": settlement.requested_minor,
            "captured_minor": settlement.captured_minor,
            "released_minor": settlement.released_minor,
            "additional_minor": settlement.additional_minor,
            "shortfall_minor": settlement.shortfall_minor,
        }
    })
}

fn price_snapshot(model: &JobModel, held_minor: i64) -> Value {
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

fn decrypt_provider_secret(
    state: &AppState,
    encrypted_secret: Option<&str>,
) -> Result<Value, AppError> {
    let Some(encrypted_secret) = encrypted_secret else {
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
        let auth_scheme = config
            .get("auth_scheme")
            .or_else(|| secret.get("auth_scheme"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let value = if auth_scheme.trim().is_empty() {
            api_key.trim().to_owned()
        } else {
            format!("{} {}", auth_scheme.trim(), api_key.trim())
        };
        let header_name = config
            .get("api_key_header")
            .or_else(|| secret.get("api_key_header"))
            .and_then(Value::as_str)
            .unwrap_or("authorization");
        headers.insert(
            reqwest_header_name(header_name)?,
            reqwest_header_value(&value)?,
        );
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
        headers.insert(reqwest_header_name(key)?, reqwest_header_value(value)?);
    }

    Ok(())
}

fn reqwest_header_name(value: &str) -> Result<ReqwestHeaderName, AppError> {
    let name = ReqwestHeaderName::from_bytes(value.as_bytes())
        .map_err(|_| AppError::validation_failed("ai provider header name is invalid"))?;
    if matches!(
        name.as_str(),
        "host" | "content-length" | "connection" | "transfer-encoding"
    ) {
        return Err(AppError::validation_failed(
            "ai provider header is not allowed",
        ));
    }

    Ok(name)
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

fn provider_request_id_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get("x-request-id")
        .or_else(|| headers.get("x-provider-request-id"))
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.to_owned()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
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

fn is_http_asset_url(value: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(value) else {
        return false;
    };
    matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
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

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/quicktime" => "mov",
        "video/mpeg" => "mpeg",
        "video/x-msvideo" => "avi",
        "video/x-matroska" => "mkv",
        _ => "bin",
    }
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

fn check_generation_rate_limit(
    state: &AppState,
    server_key_id: Uuid,
    customer_id: Uuid,
) -> impl std::future::Future<Output = Result<(), AppError>> + '_ {
    rate_limit::check_fixed_window(
        state,
        rate_limit::ai_gateway_key(&format!("server_job:{server_key_id}:{customer_id}")),
        state.config.security.ai_gateway_rate_limit_max,
        state.config.security.ai_gateway_rate_limit_window_seconds,
        AppError::rate_limited,
    )
}

fn normalize_status(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "pending" | "submitted" | "running" | "provider_succeeded" | "caching" | "succeeded"
        | "provider_failed" | "failed" | "timeout_review" | "cancelled" => Ok(value),
        _ => Err(AppError::validation_failed(
            "ai generation job status is invalid",
        )),
    }
}

fn normalize_job_type(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "image" | "video" => Ok(value),
        _ => Err(AppError::validation_failed(
            "ai generation job type is invalid",
        )),
    }
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
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

fn truncate_error(error: &str) -> String {
    const MAX_ERROR_LEN: usize = 2_000;
    if error.len() <= MAX_ERROR_LEN {
        return error.to_owned();
    }

    error.chars().take(MAX_ERROR_LEN).collect()
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("ai generation job database error: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        collect_asset_urls, estimated_quantity, provider_job_id_from_body, requested_video_seconds,
        wuyin_status_from_body, JobModel, ProviderJobStatus,
    };

    #[test]
    fn wuyin_status_follows_provider_code() {
        assert_eq!(
            wuyin_status_from_body(&json!({"data": {"status": 0}})),
            ProviderJobStatus::Pending
        );
        assert_eq!(
            wuyin_status_from_body(&json!({"data": {"status": 1}})),
            ProviderJobStatus::Running
        );
        assert_eq!(
            wuyin_status_from_body(&json!({"data": {"status": 2}})),
            ProviderJobStatus::Succeeded
        );
        assert_eq!(
            wuyin_status_from_body(&json!({"data": {"status": 3}})),
            ProviderJobStatus::Failed
        );
    }

    #[test]
    fn provider_job_id_accepts_common_shapes() {
        assert_eq!(
            provider_job_id_from_body(&json!({"data": {"id": "task-1"}})).expect("id"),
            "task-1"
        );
        assert_eq!(
            provider_job_id_from_body(&json!({"task_id": 123})).expect("id"),
            "123"
        );
    }

    #[test]
    fn asset_urls_are_collected_from_nested_provider_result() {
        let urls = collect_asset_urls(&json!({
            "data": {
                "result": {
                    "image_urls": ["https://cdn.example.com/a.png"],
                    "items": [{ "video_url": "https://cdn.example.com/b.mp4" }]
                }
            }
        }));

        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/a.png".to_owned(),
                "https://cdn.example.com/b.mp4".to_owned()
            ]
        );
    }

    #[test]
    fn video_duration_uses_common_fields() {
        assert_eq!(
            requested_video_seconds(&json!({"model": "x", "duration": 3.2})).expect("seconds"),
            4
        );
        assert_eq!(
            requested_video_seconds(&json!({"model": "x", "video": {"seconds": "8"}}))
                .expect("seconds"),
            8
        );
    }

    #[test]
    fn quantity_matches_job_billing_type() {
        let video = JobModel {
            id: uuid::Uuid::new_v4(),
            provider_id: uuid::Uuid::new_v4(),
            provider_kind: "wuyin_keji".to_owned(),
            provider_name: "速创".to_owned(),
            provider_base_url: "https://api.example.com".to_owned(),
            provider_config: json!({}),
            provider_secret_encrypted: Some("secret".to_owned()),
            code: "video".to_owned(),
            modality: "video".to_owned(),
            provider_model: None,
            currency: "CNY".to_owned(),
            billing_mode: "video_per_second".to_owned(),
            input_1k_price_minor: 0,
            output_1k_price_minor: 0,
            request_price_minor: 0,
            image_price_minor: 0,
            second_price_minor: 10,
            minute_price_minor: 0,
            daily_spend_limit_minor: None,
            pricing_config: json!({}),
        };
        assert_eq!(
            estimated_quantity("video", &json!({"model": "video", "duration": 8}), &video)
                .expect("quantity"),
            8
        );
    }
}
