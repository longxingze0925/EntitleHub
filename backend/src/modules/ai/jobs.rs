use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

use axum::{
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderMap, HeaderValue},
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
        ai::capabilities,
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        server_api::{
            ai_invoke_scope, authenticate_server_key, customer_id_from_headers,
            ensure_server_customer_subscription, ServerApiKeyContext,
        },
        web_assets, web_works,
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
    #[serde(rename = "assetUrls")]
    #[sqlx(rename = "asset_urls_alias")]
    pub asset_urls_alias: Value,
    pub results: Value,
    pub assets: Value,
    #[serde(rename = "workId")]
    pub work_id: Option<Uuid>,
    pub progress: i64,
    pub charge_mode: String,
    pub quantity: i64,
    pub held_minor: i64,
    pub charged_minor: i64,
    pub refunded_minor: i64,
    pub currency: String,
    pub failure_reason: Option<String>,
    #[serde(rename = "sourceMode")]
    pub source_mode: Option<String>,
    #[serde(rename = "referenceCount")]
    pub reference_count: i64,
    #[serde(rename = "hasFirstFrame")]
    pub has_first_frame: bool,
    #[serde(rename = "hasLastFrame")]
    pub has_last_frame: bool,
    pub visibility: Option<String>,
    #[serde(rename = "publishedAt")]
    pub published_at: Option<DateTime<Utc>>,
    #[serde(rename = "favoritedAt")]
    pub favorited_at: Option<DateTime<Utc>>,
    #[serde(rename = "downloadedAt")]
    pub downloaded_at: Option<DateTime<Utc>>,
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
pub struct AiGenerationJobDetailResponse {
    pub job: AiGenerationJobDetail,
}

#[derive(Debug, Serialize)]
pub struct AiGenerationJobListResponse {
    pub items: Vec<AiGenerationJob>,
    pub jobs: Vec<AiGenerationJob>,
    pub meta: ListMeta,
    pub pagination: ListMeta,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
    #[serde(rename = "pageSize")]
    pub page_size_alias: i64,
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

impl ListMeta {
    fn new(page: i64, page_size: i64, has_more: bool) -> Self {
        Self {
            page,
            page_size,
            page_size_alias: page_size,
            has_more,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AiGenerationJobListQuery {
    pub status: Option<String>,
    pub job_type: Option<String>,
    pub customer_id: Option<Uuid>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct AiGenerationJobDetail {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub job: AiGenerationJob,
    pub provider_submit_response: Option<Value>,
    pub provider_result_response: Option<Value>,
    pub request_payload: Value,
}

#[derive(Debug, Deserialize)]
pub struct AdminJobActionRequest {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebCreateGenerationJobRequest {
    pub customer_id: Uuid,
    #[serde(rename = "type")]
    pub job_type: String,
    #[serde(flatten)]
    pub payload: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
pub struct WebGenerationJobListQuery {
    pub customer_id: Uuid,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub job_type: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WebGenerationJobCustomerQuery {
    pub customer_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct WebGenerationJobActionRequest {
    pub customer_id: Uuid,
    pub reason: Option<String>,
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

#[derive(Debug, Clone, Default)]
struct GenerationInputAssets {
    input_mode: Option<String>,
    source_mode: Option<String>,
    reference_asset_ids: Vec<Uuid>,
    reference_urls: Vec<String>,
    first_frame_asset_id: Option<Uuid>,
    first_frame_url: Option<String>,
    last_frame_asset_id: Option<Uuid>,
    last_frame_url: Option<String>,
    reference_count: i64,
    has_first_frame: bool,
    has_last_frame: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceAssetRole {
    Reference,
    FirstFrame,
    LastFrame,
}

#[derive(Debug, Clone)]
struct ReferenceAssetInput {
    asset_id: Uuid,
    kind: Option<String>,
    role: ReferenceAssetRole,
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
    app_id: Option<Uuid>,
    customer_id: Uuid,
    usage_id: Option<Uuid>,
    provider_id: Option<Uuid>,
    job_type: String,
    provider_job_id: Option<String>,
    attempts: i32,
    held_minor: i64,
    charge_mode: String,
    request_payload: Value,
}

#[derive(Debug, Clone, FromRow)]
struct AdminJobRecord {
    id: Uuid,
    tenant_id: Uuid,
    customer_id: Uuid,
    usage_id: Option<Uuid>,
    provider_id: Option<Uuid>,
    job_type: String,
    status: String,
    provider_status: Option<String>,
    provider_job_id: Option<String>,
    provider_result_response: Option<Value>,
    held_minor: i64,
    charged_minor: i64,
    refunded_minor: i64,
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

pub async fn web_create_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<WebCreateGenerationJobRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    let job_type = normalize_job_type(&payload.job_type)?;
    let mut headers = headers;
    headers.insert(
        "x-entitlehub-customer-id",
        HeaderValue::from_str(&payload.customer_id.to_string()).map_err(|_| {
            AppError::validation_failed("customer_id header could not be constructed")
        })?,
    );
    create_server_generation_job(
        state,
        request_id,
        headers,
        Value::Object(payload.payload),
        &job_type,
    )
    .await
}

pub async fn web_list_jobs(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Query(query): Query<WebGenerationJobListQuery>,
) -> Result<Json<ApiResponse<AiGenerationJobListResponse>>, AppError> {
    let server_key = authenticate_server_key(&state, &headers, ai_invoke_scope()).await?;
    ensure_server_customer_subscription(&state, &server_key, query.customer_id).await?;
    let status = query.status.as_deref().map(normalize_status).transpose()?;
    let job_type = query
        .job_type
        .as_deref()
        .map(normalize_job_type)
        .transpose()?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let fetch_limit = page_size + 1;
    let items = list_generation_jobs(
        &state,
        server_key.tenant_id,
        status.as_deref(),
        job_type.as_deref(),
        Some(query.customer_id),
        page,
        fetch_limit,
    )
    .await?;

    let has_more = items.len() as i64 > page_size;
    let items = items
        .into_iter()
        .take(page_size as usize)
        .collect::<Vec<_>>();
    let meta = ListMeta::new(page, page_size, has_more);
    Ok(Json(ApiResponse::ok(
        AiGenerationJobListResponse {
            jobs: items.clone(),
            items,
            meta: meta.clone(),
            pagination: meta,
        },
        request_id.to_string(),
    )))
}

pub async fn web_get_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(job_id): Path<Uuid>,
    Query(query): Query<WebGenerationJobCustomerQuery>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    let server_key = authenticate_server_key(&state, &headers, ai_invoke_scope()).await?;
    ensure_server_customer_subscription(&state, &server_key, query.customer_id).await?;
    let job = find_generation_job(&state, server_key.tenant_id, query.customer_id, job_id).await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job },
        request_id.to_string(),
    )))
}

pub async fn web_cancel_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<WebGenerationJobActionRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    let server_key = authenticate_server_key(&state, &headers, ai_invoke_scope()).await?;
    ensure_server_customer_subscription(&state, &server_key, payload.customer_id).await?;
    let reason = normalize_action_reason(payload.reason, "Web 后端取消 AI 生成任务")?;
    cancel_server_job_and_release_usage(
        &state,
        server_key.tenant_id,
        payload.customer_id,
        job_id,
        &reason,
    )
    .await?;
    let job =
        find_generation_job(&state, server_key.tenant_id, payload.customer_id, job_id).await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job },
        request_id.to_string(),
    )))
}

pub async fn web_retry_job(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<WebGenerationJobActionRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    let server_key = authenticate_server_key(&state, &headers, ai_invoke_scope()).await?;
    ensure_server_customer_subscription(&state, &server_key, payload.customer_id).await?;
    let reason = normalize_action_reason(payload.reason, "Web 后端重新查询 AI 生成任务")?;
    let job = load_customer_job(&state, server_key.tenant_id, payload.customer_id, job_id).await?;
    ensure_job_can_retry_poll(&job)?;
    mark_job_for_poll_retry(&state, server_key.tenant_id, job_id, &reason).await?;
    let updated =
        find_generation_job(&state, server_key.tenant_id, payload.customer_id, job_id).await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job: updated },
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
    let fetch_limit = page_size + 1;
    let items = list_generation_jobs(
        &state,
        admin.tenant_id,
        status.as_deref(),
        job_type.as_deref(),
        query.customer_id,
        page,
        fetch_limit,
    )
    .await?;

    let has_more = items.len() as i64 > page_size;
    let items = items
        .into_iter()
        .take(page_size as usize)
        .collect::<Vec<_>>();
    let meta = ListMeta::new(page, page_size, has_more);
    Ok(Json(ApiResponse::ok(
        AiGenerationJobListResponse {
            jobs: items.clone(),
            items,
            meta: meta.clone(),
            pagination: meta,
        },
        request_id.to_string(),
    )))
}

pub async fn get_ai_generation_job(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AiGenerationJobDetailResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:job:read")?;
    let job = find_generation_job_detail(&state, admin.tenant_id, job_id).await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobDetailResponse { job },
        request_id.to_string(),
    )))
}

pub async fn retry_ai_generation_job_poll(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<AdminJobActionRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:job:update")?;
    let reason = normalize_action_reason(payload.reason, "后台重新查询第三方任务")?;
    let job = load_admin_job(&state, admin.tenant_id, job_id).await?;
    ensure_job_can_retry_poll(&job)?;
    mark_job_for_poll_retry(&state, admin.tenant_id, job_id, &reason).await?;
    let updated = find_generation_job_by_id(&state, admin.tenant_id, job_id).await?;
    audit_admin_job_action(
        &state,
        &admin,
        &request_id,
        "ai_generation_job.retry_poll",
        &job,
        &updated,
        &reason,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job: updated },
        request_id.to_string(),
    )))
}

pub async fn retry_ai_generation_job_cache(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<AdminJobActionRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:job:update")?;
    let reason = normalize_action_reason(payload.reason, "后台重新缓存生成素材")?;
    let job = load_admin_job(&state, admin.tenant_id, job_id).await?;
    ensure_job_can_retry_cache(&job)?;
    retry_cache_admin_job(&state, &job, &reason).await?;
    let updated = find_generation_job_by_id(&state, admin.tenant_id, job_id).await?;
    audit_admin_job_action(
        &state,
        &admin,
        &request_id,
        "ai_generation_job.retry_cache",
        &job,
        &updated,
        &reason,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job: updated },
        request_id.to_string(),
    )))
}

pub async fn fail_ai_generation_job_release(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<AdminJobActionRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:job:update")?;
    let reason = normalize_action_reason(payload.reason, "后台标记失败并释放预扣")?;
    let job = load_admin_job(&state, admin.tenant_id, job_id).await?;
    ensure_job_can_fail_release(&job)?;
    fail_admin_job_and_release_usage(&state, &job, &reason).await?;
    let updated = find_generation_job_by_id(&state, admin.tenant_id, job_id).await?;
    audit_admin_job_action(
        &state,
        &admin,
        &request_id,
        "ai_generation_job.fail_release",
        &job,
        &updated,
        &reason,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job: updated },
        request_id.to_string(),
    )))
}

pub async fn refund_ai_generation_job(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(job_id): Path<Uuid>,
    Json(payload): Json<AdminJobActionRequest>,
) -> Result<Json<ApiResponse<AiGenerationJobResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:job:update")?;
    let reason = normalize_action_reason(payload.reason, "后台人工退款")?;
    let job = load_admin_job(&state, admin.tenant_id, job_id).await?;
    ensure_job_can_refund(&job)?;
    refund_admin_job(&state, &job, &reason).await?;
    let updated = find_generation_job_by_id(&state, admin.tenant_id, job_id).await?;
    audit_admin_job_action(
        &state,
        &admin,
        &request_id,
        "ai_generation_job.refund",
        &job,
        &updated,
        &reason,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiGenerationJobResponse { job: updated },
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
    job_type: &str,
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
    let input_assets = load_and_validate_generation_input_assets(
        &state,
        &server_key,
        customer_id,
        job_type,
        &payload,
        &model,
    )
    .await?;
    let normalized_payload =
        normalize_generation_request_payload(payload, job_type, &model, &input_assets)?;
    validate_generation_payload(job_type, &normalized_payload, &model)?;
    let quantity = estimated_quantity(job_type, &normalized_payload, &model)?;
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
        &normalized_payload,
        idempotency_key.as_deref(),
    )
    .await?;
    let job_id = Uuid::new_v4();
    let submit_started = Instant::now();
    let submit_result =
        submit_provider_generation_job(&state, &model, job_type, normalized_payload.clone()).await;
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
                    job_type: job_type.to_owned(),
                    provider_status: submit.status,
                    provider_job_id: submit.provider_job_id,
                    provider_request_id: submit.provider_request_id,
                    provider_submit_response: submit.body,
                    request_payload: normalized_payload,
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
    job_type: String,
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
                &job,
                &job.job_type,
                poll.asset_urls.clone(),
                &poll.body,
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
    let is_google_omni = is_wuyin_google_omni_model(model);
    object.remove("model");
    object.remove("entitlehub_input");
    object.remove("referenceAssetIds");
    object.remove("reference_asset_ids");
    object.remove("firstFrameAssetId");
    object.remove("first_frame_asset_id");
    object.remove("lastFrameAssetId");
    object.remove("last_frame_asset_id");
    object.remove("aspectRatio");
    object.remove("durationSec");
    object.remove("inputMode");
    if let Some(provider_model) = model.provider_model.as_deref() {
        object
            .entry("model".to_owned())
            .or_insert_with(|| Value::String(provider_model.to_owned()));
    }
    let reference_urls = object.remove("reference_urls");
    let first_frame_url = object.remove("first_frame_url");
    let last_frame_url = object.remove("last_frame_url");
    if is_google_omni {
        let images = object.remove("images");
        merge_wuyin_google_omni_images(
            object,
            images,
            reference_urls,
            first_frame_url,
            last_frame_url,
        );
        if let Some(value) = object.remove("resolution") {
            object.entry("size".to_owned()).or_insert(value);
        }
    } else {
        if let Some(value) = reference_urls {
            object.entry("reference_urls".to_owned()).or_insert(value);
        }
        if let Some(value) = first_frame_url {
            object
                .entry("first_frame_url".to_owned())
                .or_insert(value.clone());
            object.entry("first_frame".to_owned()).or_insert(value);
        }
        if let Some(value) = last_frame_url {
            object
                .entry("last_frame_url".to_owned())
                .or_insert(value.clone());
            object.entry("last_frame".to_owned()).or_insert(value);
        }
    }

    Ok(payload)
}

fn is_wuyin_google_omni_model(model: &JobModel) -> bool {
    model
        .provider_model
        .as_deref()
        .unwrap_or(&model.code)
        .eq_ignore_ascii_case("google_omni")
        || model
            .provider_model
            .as_deref()
            .unwrap_or(&model.code)
            .eq_ignore_ascii_case("video_google_omni")
}

fn merge_wuyin_google_omni_images(
    object: &mut Map<String, Value>,
    images: Option<Value>,
    reference_urls: Option<Value>,
    first_frame_url: Option<Value>,
    last_frame_url: Option<Value>,
) {
    let mut urls = Vec::new();
    collect_wuyin_image_input_urls(images.as_ref(), &mut urls);
    collect_wuyin_image_input_urls(reference_urls.as_ref(), &mut urls);
    collect_wuyin_image_input_urls(first_frame_url.as_ref(), &mut urls);
    collect_wuyin_image_input_urls(last_frame_url.as_ref(), &mut urls);
    let mut unique_urls = Vec::new();
    for url in urls {
        if !unique_urls.contains(&url) {
            unique_urls.push(url);
        }
    }
    if !unique_urls.is_empty() {
        object.insert("images".to_owned(), Value::String(unique_urls.join(",")));
    }
}

fn collect_wuyin_image_input_urls(value: Option<&Value>, urls: &mut Vec<String>) {
    match value {
        Some(Value::String(value)) => {
            for item in value.split(',') {
                let item = item.trim();
                if !item.is_empty() {
                    urls.push(item.to_owned());
                }
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                collect_wuyin_image_input_urls(Some(item), urls);
            }
        }
        _ => {}
    }
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

fn provider_asset_metadata(provider_body: &Value, provider_url: &str) -> Value {
    let mut metadata = Map::new();
    if let Some(container) = find_object_containing_url(provider_body, provider_url) {
        if let Some(thumbnail_url) = first_object_string(
            container,
            &[
                "thumbnailUrl",
                "thumbnail_url",
                "thumbnail",
                "coverUrl",
                "cover_url",
                "cover",
                "posterUrl",
                "poster_url",
                "poster",
            ],
        ) {
            metadata.insert("thumbnailUrl".to_owned(), Value::String(thumbnail_url));
        }
        if let Some(duration_seconds) = first_object_positive_seconds(
            container,
            &[
                "duration",
                "durationSec",
                "duration_sec",
                "durationSeconds",
                "duration_seconds",
                "seconds",
            ],
        ) {
            metadata.insert("duration".to_owned(), json!(duration_seconds));
            metadata.insert("duration_seconds".to_owned(), json!(duration_seconds));
        }
        if let Some(width) = first_object_positive_i64(container, &["width", "w"]) {
            metadata.insert("width".to_owned(), json!(width));
        }
        if let Some(height) = first_object_positive_i64(container, &["height", "h"]) {
            metadata.insert("height".to_owned(), json!(height));
        }
    }

    Value::Object(metadata)
}

fn find_object_containing_url<'a>(
    value: &'a Value,
    provider_url: &str,
) -> Option<&'a Map<String, Value>> {
    match value {
        Value::Object(object) => {
            if let Some(nested_match) = object
                .values()
                .find_map(|nested| find_object_containing_url(nested, provider_url))
            {
                return Some(nested_match);
            }
            let contains_url = object
                .values()
                .any(|value| value_contains_string(value, provider_url));
            contains_url.then_some(object)
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_object_containing_url(item, provider_url)),
        _ => None,
    }
}

fn value_contains_string(value: &Value, expected: &str) -> bool {
    match value {
        Value::String(value) => value == expected,
        Value::Array(items) => items
            .iter()
            .any(|item| value_contains_string(item, expected)),
        Value::Object(object) => object
            .values()
            .any(|nested| value_contains_string(nested, expected)),
        _ => false,
    }
}

fn first_object_string(object: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(value_to_string)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn first_object_positive_seconds(object: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(capabilities::value_to_positive_seconds)
}

fn first_object_positive_i64(object: &Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(capabilities::value_to_positive_seconds)
}

fn merge_object_json(mut base: Value, overlay: Value) -> Value {
    let Some(overlay) = overlay.as_object() else {
        return base;
    };
    if let Some(base_object) = base.as_object_mut() {
        for (key, value) in overlay {
            base_object.insert(key.clone(), value.clone());
        }
    }
    base
}

async fn cache_generation_assets(
    state: &AppState,
    job: &JobWorkerRecord,
    job_type: &str,
    provider_urls: Vec<String>,
    provider_body: &Value,
) -> Result<Vec<String>, AppError> {
    let usage_id = job
        .usage_id
        .ok_or_else(|| AppError::dependency("ai generation usage id missing"))?;
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
        let asset_metadata = provider_asset_metadata(provider_body, &provider_url);
        let asset = download_provider_asset(&provider_url, max_bytes).await?;
        let public_url = store_ai_asset(
            state,
            job,
            usage_id,
            job_type,
            Some(provider_url),
            asset_metadata,
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
    job: &JobWorkerRecord,
    usage_id: Uuid,
    asset_type: &str,
    provider_url: Option<String>,
    asset_metadata: Value,
    mime_type: String,
    bytes: Vec<u8>,
) -> Result<String, AppError> {
    let asset_id = Uuid::new_v4();
    let extension = extension_for_mime(&mime_type);
    let storage_key = format!("tenants/{}/ai-assets/{asset_id}.{extension}", job.tenant_id);
    state.object_store.put_bytes(&storage_key, &bytes).await?;
    let checksum = format!("{:x}", Sha256::digest(&bytes));
    let public_url = asset_public_url(state, asset_id);
    let file_size = bytes.len() as i64;

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
    .bind(job.tenant_id)
    .bind(usage_id)
    .bind(asset_type)
    .bind(&provider_url)
    .bind(&storage_key)
    .bind(&public_url)
    .bind(&mime_type)
    .bind(file_size)
    .bind(&checksum)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;
    if let Some(app_id) = job.app_id {
        let input_metadata = job
            .request_payload
            .get("entitlehub_input")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let metadata = merge_object_json(
            json!({
                "source": "ai_generation_job",
                "job_id": job.id,
                "usage_id": usage_id,
                "provider_url": provider_url,
                "sourceMode": input_metadata.get("sourceMode").cloned().unwrap_or(json!(job.job_type)),
                "referenceCount": input_metadata.get("referenceCount").cloned().unwrap_or(json!(0)),
                "hasFirstFrame": input_metadata.get("hasFirstFrame").cloned().unwrap_or(json!(false)),
                "hasLastFrame": input_metadata.get("hasLastFrame").cloned().unwrap_or(json!(false)),
                "input": input_metadata,
            }),
            asset_metadata,
        );
        let customer_asset_id = web_assets::mirror_generated_ai_asset(
            state,
            job.tenant_id,
            app_id,
            job.customer_id,
            asset_id,
            asset_type,
            &public_url,
            &storage_key,
            &mime_type,
            file_size,
            &checksum,
            metadata.clone(),
        )
        .await?;
        web_works::create_work_for_generated_asset(
            state,
            job.tenant_id,
            app_id,
            job.customer_id,
            Some(job.id),
            customer_asset_id,
            asset_type,
            metadata,
        )
        .await?;
    }

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

async fn fail_admin_job_and_release_usage(
    state: &AppState,
    job: &AdminJobRecord,
    reason: &str,
) -> Result<(), AppError> {
    let usage_id = job
        .usage_id
        .ok_or_else(|| AppError::dependency("ai generation usage id missing"))?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let usage = find_usage_for_update(&mut transaction, usage_id).await?;
    let refunded_minor = if usage.status != "failed" && usage.status != "succeeded" {
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
                json!({
                    "source": "admin_job_action",
                    "job_id": job.id,
                }),
            )
            .await?;
        }
        update_usage_failed(
            &mut transaction,
            usage_id,
            job.provider_status
                .as_deref()
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0),
            job.provider_job_id.as_deref(),
            usage.held_minor,
            job.provider_result_response.as_ref(),
        )
        .await?;
        usage.held_minor
    } else {
        job.refunded_minor
    };
    update_job_failed(
        &mut transaction,
        job.id,
        "failed",
        job.provider_status.clone(),
        reason,
        job.provider_result_response.as_ref(),
        refunded_minor,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)
}

async fn cancel_server_job_and_release_usage(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    job_id: Uuid,
    reason: &str,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let job =
        load_customer_job_for_update(&mut transaction, tenant_id, customer_id, job_id).await?;
    ensure_job_can_cancel(&job)?;
    let usage_id = job
        .usage_id
        .ok_or_else(|| AppError::dependency("ai generation usage id missing"))?;
    let usage = find_usage_for_update(&mut transaction, usage_id).await?;
    let mut released_minor = 0;
    if usage.status != "failed" && usage.status != "succeeded" && usage.held_minor > 0 {
        let wallet = find_wallet_by_id_for_update(&mut transaction, usage.wallet_id).await?;
        let updated_wallet = update_wallet_hold(
            &mut transaction,
            wallet.tenant_id,
            wallet.id,
            -usage.held_minor,
        )
        .await?;
        insert_wallet_ledger_entry(
            &mut transaction,
            wallet.tenant_id,
            &updated_wallet,
            "release",
            -usage.held_minor,
            reason,
            usage_id,
            json!({
                "source": "web_job_cancel",
                "job_id": job_id,
            }),
        )
        .await?;
        update_usage_failed(
            &mut transaction,
            usage_id,
            0,
            job.provider_job_id.as_deref(),
            usage.held_minor,
            None,
        )
        .await?;
        released_minor = usage.held_minor;
    }
    mark_job_cancelled(
        &mut transaction,
        tenant_id,
        customer_id,
        job_id,
        released_minor,
        reason,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)
}

async fn refund_admin_job(
    state: &AppState,
    job: &AdminJobRecord,
    reason: &str,
) -> Result<(), AppError> {
    let usage_id = job
        .usage_id
        .ok_or_else(|| AppError::dependency("ai generation usage id missing"))?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let usage = find_usage_for_update(&mut transaction, usage_id).await?;
    if usage.status != "succeeded" {
        return Err(AppError::business_rule_failed(
            "only succeeded usage can be refunded",
        ));
    }
    let refund_minor = job.charged_minor.saturating_sub(job.refunded_minor);
    if refund_minor <= 0 {
        return Err(AppError::business_rule_failed(
            "ai generation job already refunded",
        ));
    }
    let wallet = find_wallet_by_id_for_update(&mut transaction, usage.wallet_id).await?;
    let updated_wallet = update_wallet_balance(&mut transaction, wallet.id, refund_minor).await?;
    insert_wallet_ledger_entry(
        &mut transaction,
        wallet.tenant_id,
        &updated_wallet,
        "refund",
        refund_minor,
        reason,
        usage_id,
        json!({
            "source": "admin_job_action",
            "job_id": job.id,
        }),
    )
    .await?;
    update_usage_refunded(&mut transaction, usage_id, refund_minor, reason).await?;
    update_job_refunded(&mut transaction, job.id, refund_minor, reason).await?;
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

async fn update_wallet_balance(
    transaction: &mut Transaction<'_, Postgres>,
    wallet_id: Uuid,
    delta_minor: i64,
) -> Result<WalletRecord, AppError> {
    sqlx::query_as::<_, WalletRecord>(
        r#"
        update ai_wallets
        set balance_minor = balance_minor + $2,
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
    .bind(delta_minor)
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

async fn update_usage_refunded(
    transaction: &mut Transaction<'_, Postgres>,
    usage_id: Uuid,
    refund_minor: i64,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_usage_records
        set status = 'refunded',
            refunded_minor = refunded_minor + $2,
            metadata_json = metadata_json || $3::jsonb,
            completed_at = now()
        where id = $1
        "#,
    )
    .bind(usage_id)
    .bind(refund_minor)
    .bind(json!({
        "manual_refund": {
            "amount_minor": refund_minor,
            "reason": reason,
            "refunded_at": Utc::now(),
        }
    }))
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
          (
            select k.app_id
            from server_api_keys k
            where k.id = ai_generation_jobs.server_key_id
          ) as app_id,
          customer_id,
          usage_id,
          provider_id,
          job_type,
          provider_job_id,
          attempts,
          held_minor,
          charge_mode,
          request_payload
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

async fn mark_job_for_poll_retry(
    state: &AppState,
    tenant_id: Uuid,
    job_id: Uuid,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'running',
            failure_reason = $3,
            next_poll_at = now(),
            updated_at = now()
        where tenant_id = $1
          and id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(job_id)
    .bind(truncate_error(reason))
    .execute(&state.db)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn retry_cache_admin_job(
    state: &AppState,
    job: &AdminJobRecord,
    reason: &str,
) -> Result<(), AppError> {
    let provider_body = job.provider_result_response.clone().ok_or_else(|| {
        AppError::business_rule_failed("ai generation job has no provider result")
    })?;
    let asset_urls = collect_asset_urls(&provider_body);
    if asset_urls.is_empty() {
        return Err(AppError::business_rule_failed(
            "ai generation job provider result has no asset urls",
        ));
    }
    mark_job_provider_succeeded(state, job.id, job.provider_status.clone(), &provider_body).await?;
    let cached_urls = cache_generation_assets(
        state,
        &JobWorkerRecord {
            id: job.id,
            tenant_id: job.tenant_id,
            app_id: app_id_for_admin_job(state, job).await?,
            customer_id: job.customer_id,
            usage_id: job.usage_id,
            provider_id: job.provider_id,
            job_type: job.job_type.clone(),
            provider_job_id: job.provider_job_id.clone(),
            attempts: 0,
            held_minor: job.held_minor,
            charge_mode: job.charge_mode.clone(),
            request_payload: json!({}),
        },
        &job.job_type,
        asset_urls,
        &provider_body,
    )
    .await?;
    let charged_minor = charge_minor_from_admin_job(job);
    capture_usage(
        state,
        job.usage_id,
        charged_minor,
        &json!({
            "manual_retry_cache": true,
            "reason": reason,
            "provider": provider_body,
        }),
        job.provider_job_id.as_deref().unwrap_or("manual-cache"),
    )
    .await?;
    mark_job_succeeded(
        state,
        job.id,
        charged_minor,
        cached_urls,
        job.provider_status.clone(),
        provider_body,
    )
    .await
}

async fn app_id_for_admin_job(
    state: &AppState,
    job: &AdminJobRecord,
) -> Result<Option<Uuid>, AppError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        select k.app_id
        from ai_generation_jobs j
        join server_api_keys k
          on k.id = j.server_key_id
        where j.tenant_id = $1
          and j.id = $2
        "#,
    )
    .bind(job.tenant_id)
    .bind(job.id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)
}

async fn update_job_refunded(
    transaction: &mut Transaction<'_, Postgres>,
    job_id: Uuid,
    refund_minor: i64,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'failed',
            refunded_minor = refunded_minor + $2,
            failure_reason = $3,
            next_poll_at = null,
            completed_at = now(),
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(job_id)
    .bind(refund_minor)
    .bind(truncate_error(reason))
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn mark_job_cancelled(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    job_id: Uuid,
    released_minor: i64,
    reason: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update ai_generation_jobs
        set status = 'cancelled',
            refunded_minor = refunded_minor + $4,
            failure_reason = $5,
            next_poll_at = null,
            completed_at = now(),
            updated_at = now()
        where tenant_id = $1
          and customer_id = $2
          and id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(job_id)
    .bind(released_minor)
    .bind(truncate_error(reason))
    .execute(&mut **transaction)
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

async fn load_customer_job(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    job_id: Uuid,
) -> Result<AdminJobRecord, AppError> {
    sqlx::query_as::<_, AdminJobRecord>(
        r#"
        select
          id,
          tenant_id,
          customer_id,
          usage_id,
          provider_id,
          job_type,
          status,
          provider_status,
          provider_job_id,
          provider_result_response,
          held_minor,
          charged_minor,
          refunded_minor,
          charge_mode
        from ai_generation_jobs
        where tenant_id = $1
          and customer_id = $2
          and id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(job_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai generation job not found"))
}

async fn load_customer_job_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
    job_id: Uuid,
) -> Result<AdminJobRecord, AppError> {
    sqlx::query_as::<_, AdminJobRecord>(
        r#"
        select
          id,
          tenant_id,
          customer_id,
          usage_id,
          provider_id,
          job_type,
          status,
          provider_status,
          provider_job_id,
          provider_result_response,
          held_minor,
          charged_minor,
          refunded_minor,
          charge_mode
        from ai_generation_jobs
        where tenant_id = $1
          and customer_id = $2
          and id = $3
        for update
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(job_id)
    .fetch_optional(&mut **transaction)
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

async fn find_generation_job_detail(
    state: &AppState,
    tenant_id: Uuid,
    job_id: Uuid,
) -> Result<AiGenerationJobDetail, AppError> {
    sqlx::query_as::<_, AiGenerationJobDetail>(
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
          j.asset_urls as asset_urls_alias,
          case
            when j.result_json is null then '[]'::jsonb
            when jsonb_typeof(j.result_json) = 'array' then j.result_json
            else jsonb_build_array(j.result_json)
          end as results,
          (
            select coalesce(
              jsonb_agg(
                jsonb_build_object(
                  'id', ca.id,
                  'name', ca.name,
                  'kind', ca.asset_type,
                  'asset_type', ca.asset_type,
                  'status', ca.status,
                  'url', ca.public_url,
                  'public_url', ca.public_url,
                  'mimeType', ca.mime_type,
                  'mime_type', ca.mime_type,
                  'thumbnailUrl', coalesce(
                    nullif(ca.metadata_json->>'thumbnailUrl', ''),
                    nullif(ca.metadata_json->>'thumbnail_url', ''),
                    nullif(ca.metadata_json->>'coverUrl', ''),
                    nullif(ca.metadata_json->>'cover_url', ''),
                    nullif(ca.metadata_json->>'posterUrl', ''),
                    nullif(ca.metadata_json->>'poster_url', ''),
                    case when ca.asset_type = 'image' then ca.public_url else null end
                  ),
                  'durationSec', coalesce(
                    ca.metadata_json->'durationSec',
                    ca.metadata_json->'durationSeconds',
                    ca.metadata_json->'duration',
                    ca.metadata_json->'duration_seconds',
                    ca.metadata_json->'seconds'
                  ),
                  'durationSeconds', coalesce(
                    ca.metadata_json->'durationSeconds',
                    ca.metadata_json->'durationSec',
                    ca.metadata_json->'duration',
                    ca.metadata_json->'duration_seconds',
                    ca.metadata_json->'seconds'
                  ),
                  'source', case
                    when ca.source = 'generated' then 'ai'
                    when ca.source = 'user_upload' then 'upload'
                    else ca.source
                  end,
                  'sourceAlias', case
                    when ca.source = 'generated' then 'ai'
                    when ca.source = 'user_upload' then 'upload'
                    else ca.source
                  end,
                  'createdAt', ca.created_at
                )
                order by ca.created_at, ca.id
              ),
              '[]'::jsonb
            )
            from customer_assets ca
            where ca.tenant_id = j.tenant_id
              and ca.customer_id = j.customer_id
              and ca.deleted_at is null
              and ca.metadata_json->>'job_id' = j.id::text
          ) as assets,
          work.id as work_id,
          case
            when j.status in ('succeeded', 'failed', 'cancelled', 'refunded') then 100
            when j.status = 'caching' then 90
            when j.status = 'timeout_review' then 95
            when j.status = 'running' then 50
            when j.status in ('submitted', 'pending') then 10
            else 0
          end as progress,
          j.charge_mode,
          j.quantity,
          j.held_minor,
          j.charged_minor,
          j.refunded_minor,
          j.currency,
          j.failure_reason,
          nullif(coalesce(work.metadata_json->>'sourceMode', j.request_payload->'entitlehub_input'->>'sourceMode'), '') as source_mode,
          coalesce(
            case when work.metadata_json->>'referenceCount' ~ '^[0-9]+$'
              then (work.metadata_json->>'referenceCount')::bigint
            end,
            case when j.request_payload->'entitlehub_input'->>'referenceCount' ~ '^[0-9]+$'
              then (j.request_payload->'entitlehub_input'->>'referenceCount')::bigint
            end,
            0
          )::bigint as reference_count,
          coalesce(
            case when lower(work.metadata_json->>'hasFirstFrame') in ('true', 'false')
              then (work.metadata_json->>'hasFirstFrame')::boolean
            end,
            case when lower(j.request_payload->'entitlehub_input'->>'hasFirstFrame') in ('true', 'false')
              then (j.request_payload->'entitlehub_input'->>'hasFirstFrame')::boolean
            end,
            false
          ) as has_first_frame,
          coalesce(
            case when lower(work.metadata_json->>'hasLastFrame') in ('true', 'false')
              then (work.metadata_json->>'hasLastFrame')::boolean
            end,
            case when lower(j.request_payload->'entitlehub_input'->>'hasLastFrame') in ('true', 'false')
              then (j.request_payload->'entitlehub_input'->>'hasLastFrame')::boolean
            end,
            false
          ) as has_last_frame,
          work.visibility,
          publication.published_at,
          favorite.created_at as favorited_at,
          download.downloaded_at,
          j.attempts,
          j.next_poll_at,
          j.submitted_at,
          j.completed_at,
          j.created_at,
          j.updated_at,
          j.provider_submit_response,
          j.provider_result_response,
          j.request_payload
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
        left join customer_works work
          on work.tenant_id = j.tenant_id
          and work.source_job_id = j.id
          and work.owner_customer_id = j.customer_id
          and work.deleted_at is null
          and work.status = 'active'
        left join work_publications publication
          on publication.tenant_id = work.tenant_id
          and publication.app_id = work.app_id
          and publication.work_id = work.id
          and publication.status = 'published'
        left join work_favorites favorite
          on favorite.tenant_id = work.tenant_id
          and favorite.app_id = work.app_id
          and favorite.work_id = work.id
          and favorite.customer_id = j.customer_id
          and favorite.deleted_at is null
        left join work_downloads download
          on download.tenant_id = work.tenant_id
          and download.app_id = work.app_id
          and download.work_id = work.id
          and download.customer_id = j.customer_id
        where j.tenant_id = $1
          and j.id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(job_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai generation job not found"))
}

async fn load_admin_job(
    state: &AppState,
    tenant_id: Uuid,
    job_id: Uuid,
) -> Result<AdminJobRecord, AppError> {
    sqlx::query_as::<_, AdminJobRecord>(
        r#"
        select
          id,
          tenant_id,
          customer_id,
          usage_id,
          provider_id,
          job_type,
          status,
          provider_status,
          provider_job_id,
          provider_result_response,
          held_minor,
          charged_minor,
          refunded_minor,
          charge_mode
        from ai_generation_jobs
        where tenant_id = $1
          and id = $2
        "#,
    )
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
      j.asset_urls as asset_urls_alias,
      case
        when j.result_json is null then '[]'::jsonb
        when jsonb_typeof(j.result_json) = 'array' then j.result_json
        else jsonb_build_array(j.result_json)
      end as results,
      (
        select coalesce(
          jsonb_agg(
            jsonb_build_object(
              'id', ca.id,
              'name', ca.name,
              'kind', ca.asset_type,
              'asset_type', ca.asset_type,
              'status', ca.status,
              'url', ca.public_url,
              'public_url', ca.public_url,
              'mimeType', ca.mime_type,
              'mime_type', ca.mime_type,
              'thumbnailUrl', coalesce(
                nullif(ca.metadata_json->>'thumbnailUrl', ''),
                nullif(ca.metadata_json->>'thumbnail_url', ''),
                nullif(ca.metadata_json->>'coverUrl', ''),
                nullif(ca.metadata_json->>'cover_url', ''),
                nullif(ca.metadata_json->>'posterUrl', ''),
                nullif(ca.metadata_json->>'poster_url', ''),
                case when ca.asset_type = 'image' then ca.public_url else null end
              ),
              'durationSec', coalesce(
                ca.metadata_json->'durationSec',
                ca.metadata_json->'durationSeconds',
                ca.metadata_json->'duration',
                ca.metadata_json->'duration_seconds',
                ca.metadata_json->'seconds'
              ),
              'durationSeconds', coalesce(
                ca.metadata_json->'durationSeconds',
                ca.metadata_json->'durationSec',
                ca.metadata_json->'duration',
                ca.metadata_json->'duration_seconds',
                ca.metadata_json->'seconds'
              ),
              'source', case
                when ca.source = 'generated' then 'ai'
                when ca.source = 'user_upload' then 'upload'
                else ca.source
              end,
              'sourceAlias', case
                when ca.source = 'generated' then 'ai'
                when ca.source = 'user_upload' then 'upload'
                else ca.source
              end,
              'createdAt', ca.created_at
            )
            order by ca.created_at, ca.id
          ),
          '[]'::jsonb
        )
        from customer_assets ca
        where ca.tenant_id = j.tenant_id
          and ca.customer_id = j.customer_id
          and ca.deleted_at is null
          and ca.metadata_json->>'job_id' = j.id::text
      ) as assets,
      work.id as work_id,
      case
        when j.status in ('succeeded', 'failed', 'cancelled', 'refunded') then 100
        when j.status = 'caching' then 90
        when j.status = 'timeout_review' then 95
        when j.status = 'running' then 50
        when j.status in ('submitted', 'pending') then 10
        else 0
      end as progress,
      j.charge_mode,
      j.quantity,
      j.held_minor,
      j.charged_minor,
      j.refunded_minor,
      j.currency,
      j.failure_reason,
      nullif(coalesce(work.metadata_json->>'sourceMode', j.request_payload->'entitlehub_input'->>'sourceMode'), '') as source_mode,
      coalesce(
        case when work.metadata_json->>'referenceCount' ~ '^[0-9]+$'
          then (work.metadata_json->>'referenceCount')::bigint
        end,
        case when j.request_payload->'entitlehub_input'->>'referenceCount' ~ '^[0-9]+$'
          then (j.request_payload->'entitlehub_input'->>'referenceCount')::bigint
        end,
        0
      )::bigint as reference_count,
      coalesce(
        case when lower(work.metadata_json->>'hasFirstFrame') in ('true', 'false')
          then (work.metadata_json->>'hasFirstFrame')::boolean
        end,
        case when lower(j.request_payload->'entitlehub_input'->>'hasFirstFrame') in ('true', 'false')
          then (j.request_payload->'entitlehub_input'->>'hasFirstFrame')::boolean
        end,
        false
      ) as has_first_frame,
      coalesce(
        case when lower(work.metadata_json->>'hasLastFrame') in ('true', 'false')
          then (work.metadata_json->>'hasLastFrame')::boolean
        end,
        case when lower(j.request_payload->'entitlehub_input'->>'hasLastFrame') in ('true', 'false')
          then (j.request_payload->'entitlehub_input'->>'hasLastFrame')::boolean
        end,
        false
      ) as has_last_frame,
      work.visibility,
      publication.published_at,
      favorite.created_at as favorited_at,
      download.downloaded_at,
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
    left join customer_works work
      on work.tenant_id = j.tenant_id
      and work.source_job_id = j.id
      and work.owner_customer_id = j.customer_id
      and work.deleted_at is null
      and work.status = 'active'
    left join work_publications publication
      on publication.tenant_id = work.tenant_id
      and publication.app_id = work.app_id
      and publication.work_id = work.id
      and publication.status = 'published'
    left join work_favorites favorite
      on favorite.tenant_id = work.tenant_id
      and favorite.app_id = work.app_id
      and favorite.work_id = work.id
      and favorite.customer_id = j.customer_id
      and favorite.deleted_at is null
    left join work_downloads download
      on download.tenant_id = work.tenant_id
      and download.app_id = work.app_id
      and download.work_id = work.id
      and download.customer_id = j.customer_id
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
        "image" => capabilities::image_count(payload, MAX_IMAGE_COUNT),
        "video" if model.billing_mode == "video_per_second" => {
            capabilities::requested_video_seconds(payload, &model.pricing_config, MAX_VIDEO_SECONDS)
        }
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

fn charge_minor_from_admin_job(job: &AdminJobRecord) -> i64 {
    match job.charge_mode.as_str() {
        "per_image" | "video_per_second" | "video_per_request" => job.held_minor,
        _ => job.held_minor,
    }
}

fn validate_generation_payload(
    job_type: &str,
    payload: &Value,
    model: &JobModel,
) -> Result<(), AppError> {
    match job_type {
        "image" => {
            capabilities::validate_image_payload(payload, &model.pricing_config, MAX_IMAGE_COUNT)
        }
        "video" => capabilities::validate_video_payload(payload, &model.pricing_config),
        _ => Err(AppError::validation_failed(
            "ai generation job type invalid",
        )),
    }
}

async fn load_and_validate_generation_input_assets(
    state: &AppState,
    server_key: &ServerApiKeyContext,
    customer_id: Uuid,
    job_type: &str,
    payload: &Value,
    model: &JobModel,
) -> Result<GenerationInputAssets, AppError> {
    let capabilities = capabilities::ModelCapabilities::from_config(&model.pricing_config)?;
    let input_mode = optional_string(payload, &["inputMode", "input_mode"]);
    if let Some(input_mode) = input_mode.as_deref() {
        capabilities::validate_input_mode(input_mode, &model.pricing_config)?;
    }
    let reference_assets = reference_asset_inputs(payload)?;
    let mut reference_asset_ids =
        uuid_list(payload, &["referenceAssetIds", "reference_asset_ids"])?;
    let mut reference_asset_kinds = reference_asset_ids
        .iter()
        .map(|asset_id| (*asset_id, None))
        .collect::<Vec<_>>();
    let mut first_frame_asset_id =
        optional_uuid(payload, &["firstFrameAssetId", "first_frame_asset_id"])?;
    let mut last_frame_asset_id =
        optional_uuid(payload, &["lastFrameAssetId", "last_frame_asset_id"])?;
    merge_reference_asset_inputs(
        reference_assets,
        &mut reference_asset_ids,
        &mut reference_asset_kinds,
        &mut first_frame_asset_id,
        &mut last_frame_asset_id,
    )?;
    if job_type != "video"
        && (!reference_asset_ids.is_empty()
            || first_frame_asset_id.is_some()
            || last_frame_asset_id.is_some())
    {
        return Err(AppError::validation_failed(
            "reference assets are only supported for video generation jobs",
        ));
    }
    if let Some(max_reference_images) = capabilities.max_reference_images {
        if reference_asset_ids.len() as i64 > max_reference_images {
            return Err(AppError::validation_failed(format!(
                "reference_asset_too_many: reference assets must contain no more than {max_reference_images} items"
            )));
        }
    }
    if first_frame_asset_id.is_some() && !capabilities.supports_first_frame {
        return Err(AppError::validation_failed(
            "model_not_support_first_frame: model does not support first frame input",
        ));
    }
    if last_frame_asset_id.is_some() && !capabilities.supports_last_frame {
        return Err(AppError::validation_failed(
            "model_not_support_last_frame: model does not support last frame input",
        ));
    }

    let mut reference_urls = Vec::new();
    for asset_id in &reference_asset_ids {
        let asset = web_assets::find_generation_asset(
            state,
            server_key.tenant_id,
            server_key.app_id,
            customer_id,
            *asset_id,
        )
        .await?;
        if let Some(kind) = expected_reference_asset_kind(&reference_asset_kinds, *asset_id) {
            validate_asset_kind_matches(&asset, kind, "referenceAssets")?;
        }
        validate_reference_asset(&asset, &capabilities, "referenceAssetIds")?;
        reference_urls.push(asset_url(&asset, "referenceAssetIds")?);
    }
    let first_frame_url = match first_frame_asset_id {
        Some(asset_id) => {
            let asset = web_assets::find_generation_asset(
                state,
                server_key.tenant_id,
                server_key.app_id,
                customer_id,
                asset_id,
            )
            .await?;
            validate_frame_asset(&asset, &capabilities, "firstFrameAssetId")?;
            Some(asset_url(&asset, "firstFrameAssetId")?)
        }
        None => None,
    };
    let last_frame_url = match last_frame_asset_id {
        Some(asset_id) => {
            let asset = web_assets::find_generation_asset(
                state,
                server_key.tenant_id,
                server_key.app_id,
                customer_id,
                asset_id,
            )
            .await?;
            validate_frame_asset(&asset, &capabilities, "lastFrameAssetId")?;
            Some(asset_url(&asset, "lastFrameAssetId")?)
        }
        None => None,
    };
    let source_mode = input_mode.clone().or_else(|| {
        if first_frame_asset_id.is_some() || last_frame_asset_id.is_some() {
            Some("frames".to_owned())
        } else if !reference_asset_ids.is_empty() {
            Some("image".to_owned())
        } else {
            Some("text".to_owned())
        }
    });

    Ok(GenerationInputAssets {
        input_mode,
        source_mode,
        reference_count: reference_asset_ids.len() as i64,
        has_first_frame: first_frame_asset_id.is_some(),
        has_last_frame: last_frame_asset_id.is_some(),
        reference_asset_ids,
        reference_urls,
        first_frame_asset_id,
        first_frame_url,
        last_frame_asset_id,
        last_frame_url,
    })
}

fn normalize_generation_request_payload(
    payload: Value,
    job_type: &str,
    model: &JobModel,
    input_assets: &GenerationInputAssets,
) -> Result<Value, AppError> {
    let mut object = payload
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::validation_failed("ai generation body must be an object"))?;
    copy_string_alias(&mut object, "aspectRatio", "ratio");
    copy_string_alias(&mut object, "durationSec", "duration");
    copy_string_alias(&mut object, "inputMode", "input_mode");
    object.remove("referenceAssets");
    object.remove("reference_assets");
    if let Some(input_mode) = input_assets.input_mode.as_deref() {
        object.insert(
            "input_mode".to_owned(),
            Value::String(input_mode.to_owned()),
        );
    }
    if !input_assets.reference_asset_ids.is_empty() {
        object.insert(
            "reference_asset_ids".to_owned(),
            json!(input_assets.reference_asset_ids),
        );
        object.insert(
            "reference_urls".to_owned(),
            json!(input_assets.reference_urls),
        );
    }
    if let Some(asset_id) = input_assets.first_frame_asset_id {
        object.insert("first_frame_asset_id".to_owned(), json!(asset_id));
    }
    if let Some(url) = input_assets.first_frame_url.as_deref() {
        object.insert("first_frame_url".to_owned(), Value::String(url.to_owned()));
    }
    if let Some(asset_id) = input_assets.last_frame_asset_id {
        object.insert("last_frame_asset_id".to_owned(), json!(asset_id));
    }
    if let Some(url) = input_assets.last_frame_url.as_deref() {
        object.insert("last_frame_url".to_owned(), Value::String(url.to_owned()));
    }
    object.insert(
        "entitlehub_input".to_owned(),
        json!({
            "sourceMode": input_assets.source_mode,
            "referenceCount": input_assets.reference_count,
            "hasFirstFrame": input_assets.has_first_frame,
            "hasLastFrame": input_assets.has_last_frame,
        }),
    );
    if job_type == "video" && !object.contains_key("duration") {
        let seconds = capabilities::requested_video_seconds(
            &Value::Object(object.clone()),
            &model.pricing_config,
            MAX_VIDEO_SECONDS,
        )?;
        object.insert("duration".to_owned(), json!(seconds));
    }

    Ok(Value::Object(object))
}

fn validate_reference_asset(
    asset: &web_assets::GenerationAssetRecord,
    capabilities: &capabilities::ModelCapabilities,
    field: &str,
) -> Result<(), AppError> {
    let Some(mime_type) = asset.mime_type.as_deref() else {
        return Err(AppError::validation_failed(format!(
            "{field} mime type is missing"
        )));
    };
    if !matches!(asset.asset_type.as_str(), "image" | "video") {
        return Err(AppError::validation_failed(format!(
            "reference_asset_kind_not_allowed: {field} must be an image or video asset"
        )));
    }
    if asset.asset_type == "video" && !capabilities.supports_reference_video {
        return Err(AppError::validation_failed(
            "model_not_support_reference_video",
        ));
    }
    validate_asset_mime_and_size(asset, capabilities, field, mime_type)
}

fn validate_frame_asset(
    asset: &web_assets::GenerationAssetRecord,
    capabilities: &capabilities::ModelCapabilities,
    field: &str,
) -> Result<(), AppError> {
    let Some(mime_type) = asset.mime_type.as_deref() else {
        return Err(AppError::validation_failed(format!(
            "{field} mime type is missing"
        )));
    };
    if !asset.asset_type.eq_ignore_ascii_case("image") {
        return Err(AppError::validation_failed(format!(
            "reference_asset_kind_not_allowed: {field} must be an image asset"
        )));
    }
    validate_asset_mime_and_size(asset, capabilities, field, mime_type)
}

fn validate_asset_mime_and_size(
    asset: &web_assets::GenerationAssetRecord,
    capabilities: &capabilities::ModelCapabilities,
    field: &str,
    mime_type: &str,
) -> Result<(), AppError> {
    if !capabilities.accepted_mime_types.is_empty()
        && !capabilities
            .accepted_mime_types
            .iter()
            .any(|item| item.eq_ignore_ascii_case(mime_type))
    {
        return Err(AppError::validation_failed(format!(
            "reference_asset_mime_not_allowed: {field} mime type must be one of {}",
            capabilities.accepted_mime_types.join(", ")
        )));
    }
    if let (Some(max_mb), Some(file_size)) = (capabilities.max_asset_size_mb, asset.file_size) {
        let max_bytes = max_mb.saturating_mul(1024 * 1024);
        if file_size > max_bytes {
            return Err(AppError::validation_failed(format!(
                "reference_asset_too_large: {field} file size must be less than or equal to {max_mb} MB"
            )));
        }
    }

    Ok(())
}

fn expected_reference_asset_kind(kinds: &[(Uuid, Option<String>)], asset_id: Uuid) -> Option<&str> {
    kinds
        .iter()
        .find_map(|(id, kind)| (*id == asset_id).then_some(kind.as_deref()).flatten())
}

fn validate_asset_kind_matches(
    asset: &web_assets::GenerationAssetRecord,
    expected_kind: &str,
    field: &str,
) -> Result<(), AppError> {
    if asset.asset_type.eq_ignore_ascii_case(expected_kind) {
        return Ok(());
    }

    Err(AppError::validation_failed(format!(
        "reference_asset_kind_mismatch: {field} expected {expected_kind} but got {}",
        asset.asset_type
    )))
}

fn asset_url(asset: &web_assets::GenerationAssetRecord, field: &str) -> Result<String, AppError> {
    asset
        .public_url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| AppError::validation_failed(format!("{field} url is missing")))
}

fn optional_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| payload.get(*key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn optional_uuid(payload: &Value, keys: &[&str]) -> Result<Option<Uuid>, AppError> {
    let Some(value) = keys.iter().find_map(|key| payload.get(*key)) else {
        return Ok(None);
    };
    uuid_from_value(value)
}

fn uuid_list(payload: &Value, keys: &[&str]) -> Result<Vec<Uuid>, AppError> {
    let Some(value) = keys.iter().find_map(|key| payload.get(*key)) else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(AppError::validation_failed(
            "referenceAssetIds must be an array",
        ));
    };
    let mut output = Vec::new();
    for item in items {
        let Some(id) = uuid_from_value(item)? else {
            return Err(AppError::validation_failed(
                "referenceAssetIds must contain valid asset ids",
            ));
        };
        if !output.contains(&id) {
            output.push(id);
        }
    }

    Ok(output)
}

fn reference_asset_inputs(payload: &Value) -> Result<Vec<ReferenceAssetInput>, AppError> {
    let Some(value) = payload
        .get("referenceAssets")
        .or_else(|| payload.get("reference_assets"))
    else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(AppError::validation_failed(
            "referenceAssets must be an array",
        ));
    };
    let mut output = Vec::new();
    for item in items {
        let Some(object) = item.as_object() else {
            return Err(AppError::validation_failed(
                "referenceAssets items must be objects",
            ));
        };
        let asset_id = object
            .get("assetId")
            .or_else(|| object.get("asset_id"))
            .map(uuid_from_value)
            .transpose()?
            .flatten()
            .ok_or_else(|| AppError::validation_failed("referenceAssets assetId is required"))?;
        let kind = object
            .get("kind")
            .or_else(|| object.get("asset_type"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());
        if let Some(kind) = kind.as_deref() {
            validate_reference_asset_kind(kind)?;
        }
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(reference_asset_role)
            .transpose()?
            .unwrap_or(ReferenceAssetRole::Reference);
        output.push(ReferenceAssetInput {
            asset_id,
            kind,
            role,
        });
    }

    Ok(output)
}

fn validate_reference_asset_kind(kind: &str) -> Result<(), AppError> {
    match kind {
        "image" | "video" => Ok(()),
        _ => Err(AppError::validation_failed(
            "reference_asset_kind_not_allowed: kind must be image or video",
        )),
    }
}

fn reference_asset_role(value: &str) -> Result<ReferenceAssetRole, AppError> {
    match value.to_ascii_lowercase().as_str() {
        "reference" | "reference_image" | "reference_video" => Ok(ReferenceAssetRole::Reference),
        "first_frame" | "firstframe" | "first-frame" => Ok(ReferenceAssetRole::FirstFrame),
        "last_frame" | "lastframe" | "last-frame" => Ok(ReferenceAssetRole::LastFrame),
        _ => Err(AppError::validation_failed(
            "reference_asset_role_invalid: role must be reference, first_frame, or last_frame",
        )),
    }
}

fn merge_reference_asset_inputs(
    inputs: Vec<ReferenceAssetInput>,
    reference_asset_ids: &mut Vec<Uuid>,
    reference_asset_kinds: &mut Vec<(Uuid, Option<String>)>,
    first_frame_asset_id: &mut Option<Uuid>,
    last_frame_asset_id: &mut Option<Uuid>,
) -> Result<(), AppError> {
    for input in inputs {
        match input.role {
            ReferenceAssetRole::Reference => {
                if !reference_asset_ids.contains(&input.asset_id) {
                    reference_asset_ids.push(input.asset_id);
                }
                if let Some(existing) = reference_asset_kinds
                    .iter_mut()
                    .find(|(asset_id, _)| *asset_id == input.asset_id)
                {
                    if existing.1.is_none() {
                        existing.1 = input.kind;
                    }
                } else {
                    reference_asset_kinds.push((input.asset_id, input.kind));
                }
            }
            ReferenceAssetRole::FirstFrame => {
                if input.kind.as_deref().is_some_and(|kind| kind != "image") {
                    return Err(AppError::validation_failed(
                        "reference_asset_kind_not_allowed: first_frame must be image",
                    ));
                }
                if first_frame_asset_id.is_some_and(|existing| existing != input.asset_id) {
                    return Err(AppError::validation_failed(
                        "reference_asset_conflict: first frame asset is duplicated",
                    ));
                }
                *first_frame_asset_id = Some(input.asset_id);
            }
            ReferenceAssetRole::LastFrame => {
                if input.kind.as_deref().is_some_and(|kind| kind != "image") {
                    return Err(AppError::validation_failed(
                        "reference_asset_kind_not_allowed: last_frame must be image",
                    ));
                }
                if last_frame_asset_id.is_some_and(|existing| existing != input.asset_id) {
                    return Err(AppError::validation_failed(
                        "reference_asset_conflict: last frame asset is duplicated",
                    ));
                }
                *last_frame_asset_id = Some(input.asset_id);
            }
        }
    }

    Ok(())
}

fn uuid_from_value(value: &Value) -> Result<Option<Uuid>, AppError> {
    let Some(value) = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    Uuid::parse_str(value)
        .map(Some)
        .map_err(|_| AppError::validation_failed("asset id is invalid"))
}

fn copy_string_alias(object: &mut Map<String, Value>, source: &str, target: &str) {
    if object.contains_key(target) {
        return;
    }
    if let Some(value) = object
        .get(source)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        object.insert(target.to_owned(), Value::String(value.to_owned()));
    }
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

fn normalize_action_reason(
    value: Option<String>,
    default_reason: &str,
) -> Result<String, AppError> {
    let reason = value.unwrap_or_else(|| default_reason.to_owned());
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > 500 || reason.contains('\0') {
        return Err(AppError::validation_failed(
            "ai generation job action reason is invalid",
        ));
    }

    Ok(reason.to_owned())
}

fn ensure_job_can_retry_poll(job: &AdminJobRecord) -> Result<(), AppError> {
    if job.provider_job_id.as_deref().is_none_or(str::is_empty) {
        return Err(AppError::business_rule_failed(
            "ai generation job has no provider job id",
        ));
    }
    match job.status.as_str() {
        "submitted" | "running" | "caching" | "timeout_review" => Ok(()),
        _ => Err(AppError::business_rule_failed(
            "ai generation job cannot be queried again in this status",
        )),
    }
}

fn ensure_job_can_cancel(job: &AdminJobRecord) -> Result<(), AppError> {
    match job.status.as_str() {
        "pending" | "submitted" | "running" | "provider_succeeded" | "caching"
        | "timeout_review" => Ok(()),
        "cancelled" => Err(AppError::conflict("ai generation job already cancelled")),
        _ => Err(AppError::business_rule_failed(
            "ai generation job cannot be cancelled in this status",
        )),
    }
}

fn ensure_job_can_retry_cache(job: &AdminJobRecord) -> Result<(), AppError> {
    if job.charged_minor > 0 {
        return Err(AppError::business_rule_failed(
            "ai generation job has already been charged",
        ));
    }
    match job.status.as_str() {
        "caching" | "timeout_review" | "failed" | "provider_failed" => Ok(()),
        _ => Err(AppError::business_rule_failed(
            "ai generation job cannot be cached again in this status",
        )),
    }
}

fn ensure_job_can_fail_release(job: &AdminJobRecord) -> Result<(), AppError> {
    if job.charged_minor > 0 {
        return Err(AppError::business_rule_failed(
            "charged ai generation job must be refunded instead",
        ));
    }
    match job.status.as_str() {
        "submitted" | "running" | "caching" | "timeout_review" | "provider_failed" | "failed" => {
            Ok(())
        }
        _ => Err(AppError::business_rule_failed(
            "ai generation job cannot be marked failed in this status",
        )),
    }
}

fn ensure_job_can_refund(job: &AdminJobRecord) -> Result<(), AppError> {
    if job.status != "succeeded" {
        return Err(AppError::business_rule_failed(
            "only succeeded ai generation jobs can be refunded",
        ));
    }
    if job.charged_minor <= job.refunded_minor {
        return Err(AppError::business_rule_failed(
            "ai generation job has no refundable charge",
        ));
    }

    Ok(())
}

fn admin_job_audit_json(job: &AdminJobRecord) -> Value {
    json!({
        "id": job.id,
        "customer_id": job.customer_id,
        "usage_id": job.usage_id,
        "provider_id": job.provider_id,
        "job_type": job.job_type,
        "status": job.status,
        "provider_status": job.provider_status,
        "provider_job_id": job.provider_job_id,
        "held_minor": job.held_minor,
        "charged_minor": job.charged_minor,
        "refunded_minor": job.refunded_minor,
        "charge_mode": job.charge_mode,
    })
}

fn generation_job_audit_json(job: &AiGenerationJob) -> Value {
    json!({
        "id": job.id,
        "customer_id": job.customer_id,
        "usage_id": job.usage_id,
        "job_type": job.job_type,
        "status": job.status,
        "provider_status": job.provider_status,
        "provider_job_id": job.provider_job_id,
        "held_minor": job.held_minor,
        "charged_minor": job.charged_minor,
        "refunded_minor": job.refunded_minor,
        "charge_mode": job.charge_mode,
        "failure_reason": job.failure_reason,
    })
}

async fn audit_admin_job_action(
    state: &AppState,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: &AdminJobRecord,
    after: &AiGenerationJob,
    reason: &str,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "ai_generation_job",
            resource_id: Some(before.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(admin_job_audit_json(before)),
            after_json: Some(generation_job_audit_json(after)),
            metadata_json: json!({ "reason": reason }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)
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

    use crate::modules::ai::capabilities;

    use super::{
        collect_asset_urls, estimated_quantity, normalize_generation_request_payload,
        provider_asset_metadata, provider_job_id_from_body, reference_asset_inputs,
        wuyin_status_from_body, wuyin_submit_payload, GenerationInputAssets, JobModel,
        ProviderJobStatus, ReferenceAssetRole, MAX_VIDEO_SECONDS,
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
    fn provider_asset_metadata_follows_matching_result_item() {
        let metadata = provider_asset_metadata(
            &json!({
                "data": {
                    "items": [
                        {
                            "video_url": "https://cdn.example.com/a.mp4",
                            "cover_url": "https://cdn.example.com/a.jpg",
                            "duration_seconds": 8.2
                        }
                    ]
                }
            }),
            "https://cdn.example.com/a.mp4",
        );

        assert_eq!(
            metadata["thumbnailUrl"],
            json!("https://cdn.example.com/a.jpg")
        );
        assert_eq!(metadata["duration"], json!(9));
        assert_eq!(metadata["duration_seconds"], json!(9));
    }

    #[test]
    fn reference_assets_accept_structured_inputs() {
        let asset_id = uuid::Uuid::new_v4();
        let inputs = reference_asset_inputs(&json!({
            "referenceAssets": [
                {
                    "assetId": asset_id,
                    "kind": "image",
                    "role": "first_frame"
                }
            ]
        }))
        .expect("reference assets");

        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].asset_id, asset_id);
        assert_eq!(inputs[0].kind.as_deref(), Some("image"));
        assert_eq!(inputs[0].role, ReferenceAssetRole::FirstFrame);
    }

    #[test]
    fn video_duration_uses_common_fields() {
        assert_eq!(
            capabilities::requested_video_seconds(
                &json!({"model": "x", "duration": 3.2}),
                &json!({}),
                MAX_VIDEO_SECONDS
            )
            .expect("seconds"),
            4
        );
        assert_eq!(
            capabilities::requested_video_seconds(
                &json!({"model": "x", "video": {"seconds": "8"}}),
                &json!({}),
                MAX_VIDEO_SECONDS
            )
            .expect("seconds"),
            8
        );
    }

    #[test]
    fn google_omni_payload_uses_wuyin_images_and_size_fields() {
        let model = JobModel {
            id: uuid::Uuid::new_v4(),
            provider_id: uuid::Uuid::new_v4(),
            provider_kind: "wuyin_keji".to_owned(),
            provider_name: "速创".to_owned(),
            provider_base_url: "https://api.example.com".to_owned(),
            provider_config: json!({}),
            provider_secret_encrypted: Some("secret".to_owned()),
            code: "video".to_owned(),
            modality: "video".to_owned(),
            provider_model: Some("google_omni".to_owned()),
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
        let payload = wuyin_submit_payload(
            json!({
                "model": "video",
                "prompt": "test",
                "resolution": "1280x720",
                "duration": 10,
                "reference_urls": ["https://cdn.example.com/ref.png"],
                "first_frame_url": "https://cdn.example.com/first.png",
                "last_frame_url": "https://cdn.example.com/last.png"
            }),
            &model,
        )
        .expect("payload");

        assert_eq!(payload["model"], json!("google_omni"));
        assert_eq!(payload["size"], json!("1280x720"));
        assert_eq!(payload["duration"], json!(10));
        assert_eq!(
            payload["images"],
            json!("https://cdn.example.com/ref.png,https://cdn.example.com/first.png,https://cdn.example.com/last.png")
        );
        assert!(payload.get("reference_urls").is_none());
        assert!(payload.get("first_frame_url").is_none());
        assert!(payload.get("last_frame_url").is_none());
    }

    #[test]
    fn normalized_generation_payload_removes_structured_reference_assets() {
        let model = JobModel {
            id: uuid::Uuid::new_v4(),
            provider_id: uuid::Uuid::new_v4(),
            provider_kind: "wuyin_keji".to_owned(),
            provider_name: "速创".to_owned(),
            provider_base_url: "https://api.example.com".to_owned(),
            provider_config: json!({}),
            provider_secret_encrypted: Some("secret".to_owned()),
            code: "video".to_owned(),
            modality: "video".to_owned(),
            provider_model: Some("google_omni".to_owned()),
            currency: "CNY".to_owned(),
            billing_mode: "video_per_second".to_owned(),
            input_1k_price_minor: 0,
            output_1k_price_minor: 0,
            request_price_minor: 0,
            image_price_minor: 0,
            second_price_minor: 10,
            minute_price_minor: 0,
            daily_spend_limit_minor: None,
            pricing_config: json!({"default_duration_seconds": 8}),
        };
        let asset_id = uuid::Uuid::new_v4();
        let payload = normalize_generation_request_payload(
            json!({
                "model": "video",
                "prompt": "test",
                "referenceAssets": [
                    {
                        "assetId": asset_id,
                        "kind": "image",
                        "role": "reference"
                    }
                ]
            }),
            "video",
            &model,
            &GenerationInputAssets {
                source_mode: Some("image".to_owned()),
                reference_asset_ids: vec![asset_id],
                reference_urls: vec!["https://cdn.example.com/ref.png".to_owned()],
                reference_count: 1,
                ..Default::default()
            },
        )
        .expect("payload");

        assert!(payload.get("referenceAssets").is_none());
        assert_eq!(payload["reference_asset_ids"], json!([asset_id]));
        assert_eq!(
            payload["reference_urls"],
            json!(["https://cdn.example.com/ref.png"])
        );
        assert_eq!(payload["entitlehub_input"]["referenceCount"], json!(1));
    }

    #[test]
    fn non_google_omni_payload_keeps_generic_reference_fields() {
        let model = JobModel {
            id: uuid::Uuid::new_v4(),
            provider_id: uuid::Uuid::new_v4(),
            provider_kind: "wuyin_keji".to_owned(),
            provider_name: "速创".to_owned(),
            provider_base_url: "https://api.example.com".to_owned(),
            provider_config: json!({}),
            provider_secret_encrypted: Some("secret".to_owned()),
            code: "video".to_owned(),
            modality: "video".to_owned(),
            provider_model: Some("grok_imagine".to_owned()),
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
        let payload = wuyin_submit_payload(
            json!({
                "model": "video",
                "reference_urls": ["https://cdn.example.com/ref.png"],
                "first_frame_url": "https://cdn.example.com/first.png",
                "last_frame_url": "https://cdn.example.com/last.png"
            }),
            &model,
        )
        .expect("payload");

        assert_eq!(payload["model"], json!("grok_imagine"));
        assert_eq!(
            payload["reference_urls"],
            json!(["https://cdn.example.com/ref.png"])
        );
        assert_eq!(
            payload["first_frame"],
            json!("https://cdn.example.com/first.png")
        );
        assert_eq!(
            payload["last_frame"],
            json!("https://cdn.example.com/last.png")
        );
        assert!(payload.get("images").is_none());
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
