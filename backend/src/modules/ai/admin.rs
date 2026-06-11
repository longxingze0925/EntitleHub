use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::envelope::{decrypt_bytes, encrypt_bytes, PrivateKeyEnvelope},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        ai::capabilities::validate_capabilities_config,
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const MAX_PROVIDER_NAME_LEN: usize = 128;
const MAX_MODEL_CODE_LEN: usize = 128;
const MAX_MODEL_NAME_LEN: usize = 128;
const MAX_PROVIDER_MODEL_LEN: usize = 256;
const MAX_CONFIG_BYTES: usize = 32 * 1024;
const MAX_SECRET_BYTES: usize = 32 * 1024;
const MAX_REASON_LEN: usize = 500;
const MAX_MANUAL_ADJUSTMENT_MINOR: i64 = 100_000_000_000;
const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiProvider {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: bool,
    pub config: Value,
    pub secret_configured: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AiProviderListResponse {
    pub items: Vec<AiProvider>,
}

#[derive(Debug, Serialize)]
pub struct AiProviderResponse {
    pub provider: AiProvider,
}

#[derive(Debug, Deserialize)]
pub struct AiProviderListQuery {
    pub include_history: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAiProviderRequest {
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub config: Value,
    pub secret: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAiProviderRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<Value>,
    pub secret: Option<Value>,
    #[serde(default)]
    pub clear_secret: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiModel {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub modality: String,
    pub provider_id: Option<Uuid>,
    pub provider_name: Option<String>,
    pub provider_model: Option<String>,
    pub enabled: bool,
    pub currency: String,
    pub billing_mode: String,
    pub input_1k_price_minor: i64,
    pub output_1k_price_minor: i64,
    pub request_price_minor: i64,
    pub image_price_minor: i64,
    pub second_price_minor: i64,
    pub minute_price_minor: i64,
    pub daily_spend_limit_minor: Option<i64>,
    pub pricing_config: Value,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AiModelListResponse {
    pub items: Vec<AiModel>,
}

#[derive(Debug, Serialize)]
pub struct AiModelResponse {
    pub model: AiModel,
}

#[derive(Debug, Deserialize)]
pub struct AiModelListQuery {
    pub include_history: Option<bool>,
    pub modality: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAiModelRequest {
    pub code: String,
    pub name: String,
    pub modality: String,
    pub provider_id: Option<Uuid>,
    pub provider_model: Option<String>,
    pub enabled: Option<bool>,
    pub currency: Option<String>,
    pub billing_mode: Option<String>,
    pub input_1k_price_minor: Option<i64>,
    pub output_1k_price_minor: Option<i64>,
    pub request_price_minor: Option<i64>,
    pub image_price_minor: Option<i64>,
    pub second_price_minor: Option<i64>,
    pub minute_price_minor: Option<i64>,
    pub daily_spend_limit_minor: Option<i64>,
    #[serde(default)]
    pub pricing_config: Value,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAiModelRequest {
    pub name: Option<String>,
    pub modality: Option<String>,
    pub provider_id: Option<Option<Uuid>>,
    pub provider_model: Option<Option<String>>,
    pub enabled: Option<bool>,
    pub currency: Option<String>,
    pub billing_mode: Option<String>,
    pub input_1k_price_minor: Option<i64>,
    pub output_1k_price_minor: Option<i64>,
    pub request_price_minor: Option<i64>,
    pub image_price_minor: Option<i64>,
    pub second_price_minor: Option<i64>,
    pub minute_price_minor: Option<i64>,
    pub daily_spend_limit_minor: Option<Option<i64>>,
    pub pricing_config: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiWalletSummary {
    pub customer_id: Uuid,
    pub customer_email: String,
    pub customer_name: Option<String>,
    pub wallet_id: Option<Uuid>,
    pub currency: String,
    pub balance_minor: i64,
    pub held_minor: i64,
    pub available_minor: i64,
    pub ai_enabled: bool,
    pub daily_spend_limit_minor: Option<i64>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AiWalletListResponse {
    pub items: Vec<AiWalletSummary>,
}

#[derive(Debug, Serialize)]
pub struct AiWalletResponse {
    pub wallet: AiWalletSummary,
    pub ledger_entry: Option<AiWalletLedgerEntry>,
}

#[derive(Debug, Deserialize)]
pub struct AiWalletListQuery {
    pub include_history: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct AdjustAiWalletRequest {
    pub amount_minor: i64,
    pub reason: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAiWalletQuotaRequest {
    pub daily_spend_limit_minor: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAiWalletAccessRequest {
    pub ai_enabled: bool,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiWalletLedgerEntry {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub customer_email: Option<String>,
    pub customer_name: Option<String>,
    pub currency: String,
    pub entry_type: String,
    pub amount_minor: i64,
    pub balance_after_minor: i64,
    pub held_after_minor: i64,
    pub reason: String,
    pub reference_type: Option<String>,
    pub reference_id: Option<String>,
    pub metadata: Value,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ListMeta {
    pub page: i64,
    pub page_size: i64,
}

#[derive(Debug, Serialize)]
pub struct AiWalletLedgerListResponse {
    pub items: Vec<AiWalletLedgerEntry>,
    pub meta: ListMeta,
}

#[derive(Debug, Deserialize)]
pub struct LedgerListQuery {
    pub customer_id: Option<Uuid>,
    pub entry_type: Option<String>,
    pub reference_id: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
struct AiProviderRecord {
    id: Uuid,
    name: String,
    kind: String,
    base_url: String,
    enabled: bool,
    config: Value,
    secret_encrypted: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
struct AiWalletRecord {
    id: Uuid,
    customer_id: Uuid,
    currency: String,
    balance_minor: i64,
    held_minor: i64,
    ai_enabled: bool,
    daily_spend_limit_minor: Option<i64>,
    updated_at: DateTime<Utc>,
}

struct CreateProviderInput {
    name: String,
    kind: String,
    base_url: String,
    enabled: bool,
    config: Value,
    secret_encrypted: Option<String>,
}

struct UpdateProviderInput {
    name: String,
    base_url: String,
    enabled: bool,
    config: Value,
    secret_encrypted: Option<String>,
}

struct CreateModelInput {
    code: String,
    name: String,
    modality: String,
    provider_id: Option<Uuid>,
    provider_model: Option<String>,
    enabled: bool,
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
    metadata: Value,
}

struct UpdateModelInput {
    name: String,
    modality: String,
    provider_id: Option<Uuid>,
    provider_model: Option<String>,
    enabled: bool,
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
    metadata: Value,
}

pub async fn list_ai_providers(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiProviderListQuery>,
) -> Result<Json<ApiResponse<AiProviderListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;

    let items = list_providers(
        &state,
        admin.tenant_id,
        query.include_history.unwrap_or(false),
    )
    .await?
    .into_iter()
    .map(AiProvider::from)
    .collect();

    Ok(Json(ApiResponse::ok(
        AiProviderListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn create_ai_provider(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateAiProviderRequest>,
) -> Result<Json<ApiResponse<AiProviderResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:provider:update")?;
    let input = normalize_create_provider_input(&state, payload)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let record = insert_provider(&mut transaction, admin.tenant_id, input).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_provider.create",
            resource_type: "ai_provider",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(provider_audit_json(&record)),
            metadata_json: json!({
                "name": &record.name,
                "kind": &record.kind,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AiProviderResponse {
            provider: AiProvider::from(record),
        },
        request_id.to_string(),
    )))
}

pub async fn update_ai_provider(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(provider_id): Path<Uuid>,
    Json(payload): Json<UpdateAiProviderRequest>,
) -> Result<Json<ApiResponse<AiProviderResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:provider:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_provider_for_update(&mut transaction, admin.tenant_id, provider_id).await?;
    let input = normalize_update_provider_input(&state, &before, payload)?;
    let record =
        update_provider_in_transaction(&mut transaction, admin.tenant_id, provider_id, input)
            .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_provider.update",
            resource_type: "ai_provider",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(provider_audit_json(&before)),
            after_json: Some(provider_audit_json(&record)),
            metadata_json: json!({
                "name": &record.name,
                "kind": &record.kind,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AiProviderResponse {
            provider: AiProvider::from(record),
        },
        request_id.to_string(),
    )))
}

pub async fn list_ai_models(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiModelListQuery>,
) -> Result<Json<ApiResponse<AiModelListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;
    let modality = query
        .modality
        .as_deref()
        .map(normalize_modality)
        .transpose()?;

    let items = list_models(
        &state,
        admin.tenant_id,
        query.include_history.unwrap_or(false),
        modality.as_deref(),
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiModelListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn create_ai_model(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateAiModelRequest>,
) -> Result<Json<ApiResponse<AiModelResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:model:update")?;
    let input = normalize_create_model_input(payload)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_provider_exists_in_transaction(&mut transaction, admin.tenant_id, input.provider_id)
        .await?;
    let record = insert_model(&mut transaction, admin.tenant_id, input).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_model.create",
            resource_type: "ai_model",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(model_audit_json(&record)),
            metadata_json: json!({
                "code": &record.code,
                "modality": &record.modality,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AiModelResponse { model: record },
        request_id.to_string(),
    )))
}

pub async fn update_ai_model(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(model_id): Path<Uuid>,
    Json(payload): Json<UpdateAiModelRequest>,
) -> Result<Json<ApiResponse<AiModelResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:model:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_model_for_update(&mut transaction, admin.tenant_id, model_id).await?;
    let input = normalize_update_model_input(&before, payload)?;
    ensure_provider_exists_in_transaction(&mut transaction, admin.tenant_id, input.provider_id)
        .await?;
    let record =
        update_model_in_transaction(&mut transaction, admin.tenant_id, model_id, input).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_model.update",
            resource_type: "ai_model",
            resource_id: Some(record.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(model_audit_json(&before)),
            after_json: Some(model_audit_json(&record)),
            metadata_json: json!({
                "code": &record.code,
                "modality": &record.modality,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AiModelResponse { model: record },
        request_id.to_string(),
    )))
}

pub async fn list_ai_wallets(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiWalletListQuery>,
) -> Result<Json<ApiResponse<AiWalletListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;

    let items = list_wallets(
        &state,
        admin.tenant_id,
        query.include_history.unwrap_or(false),
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiWalletListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn adjust_ai_wallet(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
    Json(payload): Json<AdjustAiWalletRequest>,
) -> Result<Json<ApiResponse<AiWalletResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:wallet:update")?;
    let amount_minor = normalize_adjustment_amount(payload.amount_minor)?;
    let reason = normalize_reason(&payload.reason)?;
    let metadata = normalize_metadata(payload.metadata)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_customer_exists_in_transaction(&mut transaction, admin.tenant_id, customer_id).await?;
    ensure_wallet_exists_in_transaction(&mut transaction, admin.tenant_id, customer_id).await?;
    let before = find_wallet_for_update(&mut transaction, admin.tenant_id, customer_id).await?;
    let updated = update_wallet_balance_in_transaction(
        &mut transaction,
        admin.tenant_id,
        before.id,
        amount_minor,
    )
    .await?;
    let entry_type = if amount_minor > 0 { "credit" } else { "debit" };
    let entry = insert_wallet_ledger_entry(
        &mut transaction,
        admin.tenant_id,
        &updated,
        entry_type,
        amount_minor,
        &reason,
        metadata,
        admin.team_member_id,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_wallet.adjust",
            resource_type: "ai_wallet",
            resource_id: Some(updated.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(wallet_audit_json(&before)),
            after_json: Some(wallet_audit_json(&updated)),
            metadata_json: json!({
                "customer_id": customer_id,
                "amount_minor": amount_minor,
                "reason": &reason,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let wallet = wallet_summary_by_customer(&state, admin.tenant_id, customer_id).await?;

    Ok(Json(ApiResponse::ok(
        AiWalletResponse {
            wallet,
            ledger_entry: Some(entry),
        },
        request_id.to_string(),
    )))
}

pub async fn update_ai_wallet_quota(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
    Json(payload): Json<UpdateAiWalletQuotaRequest>,
) -> Result<Json<ApiResponse<AiWalletResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:wallet:update")?;
    validate_optional_limit(
        payload.daily_spend_limit_minor,
        "ai wallet daily spend limit",
    )?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_customer_exists_in_transaction(&mut transaction, admin.tenant_id, customer_id).await?;
    ensure_wallet_exists_in_transaction(&mut transaction, admin.tenant_id, customer_id).await?;
    let before = find_wallet_for_update(&mut transaction, admin.tenant_id, customer_id).await?;
    let updated = update_wallet_quota_in_transaction(
        &mut transaction,
        admin.tenant_id,
        before.id,
        payload.daily_spend_limit_minor,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_wallet.quota_update",
            resource_type: "ai_wallet",
            resource_id: Some(updated.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(wallet_audit_json(&before)),
            after_json: Some(wallet_audit_json(&updated)),
            metadata_json: json!({
                "customer_id": customer_id,
                "daily_spend_limit_minor": payload.daily_spend_limit_minor,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let wallet = wallet_summary_by_customer(&state, admin.tenant_id, customer_id).await?;

    Ok(Json(ApiResponse::ok(
        AiWalletResponse {
            wallet,
            ledger_entry: None,
        },
        request_id.to_string(),
    )))
}

pub async fn update_ai_wallet_access(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
    Json(payload): Json<UpdateAiWalletAccessRequest>,
) -> Result<Json<ApiResponse<AiWalletResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:wallet:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_customer_exists_in_transaction(&mut transaction, admin.tenant_id, customer_id).await?;
    ensure_wallet_exists_in_transaction(&mut transaction, admin.tenant_id, customer_id).await?;
    let before = find_wallet_for_update(&mut transaction, admin.tenant_id, customer_id).await?;
    let updated = update_wallet_access_in_transaction(
        &mut transaction,
        admin.tenant_id,
        before.id,
        payload.ai_enabled,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: if payload.ai_enabled {
                "ai_wallet.access_enable"
            } else {
                "ai_wallet.access_freeze"
            },
            resource_type: "ai_wallet",
            resource_id: Some(updated.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(wallet_audit_json(&before)),
            after_json: Some(wallet_audit_json(&updated)),
            metadata_json: json!({
                "customer_id": customer_id,
                "ai_enabled": payload.ai_enabled,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let wallet = wallet_summary_by_customer(&state, admin.tenant_id, customer_id).await?;

    Ok(Json(ApiResponse::ok(
        AiWalletResponse {
            wallet,
            ledger_entry: None,
        },
        request_id.to_string(),
    )))
}

pub async fn list_ai_wallet_ledger(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
    Query(query): Query<LedgerListQuery>,
) -> Result<Json<ApiResponse<AiWalletLedgerListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;
    ensure_customer_exists(&state, admin.tenant_id, customer_id).await?;
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items =
        list_wallet_ledger_entries(&state, admin.tenant_id, customer_id, page, page_size).await?;

    Ok(Json(ApiResponse::ok(
        AiWalletLedgerListResponse {
            items,
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

pub async fn list_ai_wallet_ledger_entries(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<LedgerListQuery>,
) -> Result<Json<ApiResponse<AiWalletLedgerListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;
    let entry_type = clean_optional_text(query.entry_type.as_deref())
        .as_deref()
        .map(normalize_ledger_entry_type)
        .transpose()?;
    let reference_id = clean_optional_text(query.reference_id.as_deref());
    let (page, page_size) = normalize_page(query.page, query.page_size);
    let items = list_wallet_ledger_entries_for_tenant(
        &state,
        admin.tenant_id,
        query.customer_id,
        entry_type.as_deref(),
        reference_id.as_deref(),
        page,
        page_size,
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiWalletLedgerListResponse {
            items,
            meta: ListMeta { page, page_size },
        },
        request_id.to_string(),
    )))
}

async fn list_providers(
    state: &AppState,
    tenant_id: Uuid,
    include_history: bool,
) -> Result<Vec<AiProviderRecord>, AppError> {
    sqlx::query_as::<_, AiProviderRecord>(
        r#"
        select
          id,
          name,
          kind,
          base_url,
          enabled,
          config_json as config,
          secret_encrypted,
          created_at,
          updated_at
        from ai_providers
        where tenant_id = $1
          and ($2::bool or enabled)
        order by created_at desc, id desc
        "#,
    )
    .bind(tenant_id)
    .bind(include_history)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn insert_provider(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    input: CreateProviderInput,
) -> Result<AiProviderRecord, AppError> {
    sqlx::query_as::<_, AiProviderRecord>(
        r#"
        insert into ai_providers (
          tenant_id,
          name,
          kind,
          base_url,
          enabled,
          config_json,
          secret_encrypted
        )
        values ($1, $2, $3, $4, $5, $6, $7)
        returning
          id,
          name,
          kind,
          base_url,
          enabled,
          config_json as config,
          secret_encrypted,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(input.name)
    .bind(input.kind)
    .bind(input.base_url)
    .bind(input.enabled)
    .bind(input.config)
    .bind(input.secret_encrypted)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn find_provider_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    provider_id: Uuid,
) -> Result<AiProviderRecord, AppError> {
    sqlx::query_as::<_, AiProviderRecord>(
        r#"
        select
          id,
          name,
          kind,
          base_url,
          enabled,
          config_json as config,
          secret_encrypted,
          created_at,
          updated_at
        from ai_providers
        where tenant_id = $1
          and id = $2
        for update
        "#,
    )
    .bind(tenant_id)
    .bind(provider_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai provider not found"))
}

async fn update_provider_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    provider_id: Uuid,
    input: UpdateProviderInput,
) -> Result<AiProviderRecord, AppError> {
    sqlx::query_as::<_, AiProviderRecord>(
        r#"
        update ai_providers
        set
          name = $3,
          base_url = $4,
          enabled = $5,
          config_json = $6,
          secret_encrypted = $7,
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          name,
          kind,
          base_url,
          enabled,
          config_json as config,
          secret_encrypted,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(provider_id)
    .bind(input.name)
    .bind(input.base_url)
    .bind(input.enabled)
    .bind(input.config)
    .bind(input.secret_encrypted)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn list_models(
    state: &AppState,
    tenant_id: Uuid,
    include_history: bool,
    modality: Option<&str>,
) -> Result<Vec<AiModel>, AppError> {
    sqlx::query_as::<_, AiModel>(
        r#"
        select
          m.id,
          m.code,
          m.name,
          m.modality,
          m.provider_id,
          p.name as provider_name,
          m.provider_model,
          m.enabled,
          m.currency,
          m.billing_mode,
          m.input_1k_price_minor,
          m.output_1k_price_minor,
          m.request_price_minor,
          m.image_price_minor,
          m.second_price_minor,
          m.minute_price_minor,
          m.daily_spend_limit_minor,
          m.pricing_config_json as pricing_config,
          m.metadata_json as metadata,
          m.created_at,
          m.updated_at
        from ai_models m
        left join ai_providers p
          on p.tenant_id = m.tenant_id
         and p.id = m.provider_id
        where m.tenant_id = $1
          and ($2::bool or m.enabled)
          and ($3::text is null or m.modality = $3)
        order by m.created_at desc, m.id desc
        "#,
    )
    .bind(tenant_id)
    .bind(include_history)
    .bind(modality)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn insert_model(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    input: CreateModelInput,
) -> Result<AiModel, AppError> {
    sqlx::query_as::<_, AiModel>(
        r#"
        insert into ai_models (
          tenant_id,
          provider_id,
          code,
          name,
          modality,
          provider_model,
          enabled,
          currency,
          billing_mode,
          input_1k_price_minor,
          output_1k_price_minor,
          request_price_minor,
          image_price_minor,
          second_price_minor,
          minute_price_minor,
          daily_spend_limit_minor,
          pricing_config_json,
          metadata_json
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        returning
          id,
          code,
          name,
          modality,
          provider_id,
          null::text as provider_name,
          provider_model,
          enabled,
          currency,
          billing_mode,
          input_1k_price_minor,
          output_1k_price_minor,
          request_price_minor,
          image_price_minor,
          second_price_minor,
          minute_price_minor,
          daily_spend_limit_minor,
          pricing_config_json as pricing_config,
          metadata_json as metadata,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(input.provider_id)
    .bind(input.code)
    .bind(input.name)
    .bind(input.modality)
    .bind(input.provider_model)
    .bind(input.enabled)
    .bind(input.currency)
    .bind(input.billing_mode)
    .bind(input.input_1k_price_minor)
    .bind(input.output_1k_price_minor)
    .bind(input.request_price_minor)
    .bind(input.image_price_minor)
    .bind(input.second_price_minor)
    .bind(input.minute_price_minor)
    .bind(input.daily_spend_limit_minor)
    .bind(input.pricing_config)
    .bind(input.metadata)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn find_model_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    model_id: Uuid,
) -> Result<AiModel, AppError> {
    sqlx::query_as::<_, AiModel>(
        r#"
        select
          m.id,
          m.code,
          m.name,
          m.modality,
          m.provider_id,
          p.name as provider_name,
          m.provider_model,
          m.enabled,
          m.currency,
          m.billing_mode,
          m.input_1k_price_minor,
          m.output_1k_price_minor,
          m.request_price_minor,
          m.image_price_minor,
          m.second_price_minor,
          m.minute_price_minor,
          m.daily_spend_limit_minor,
          m.pricing_config_json as pricing_config,
          m.metadata_json as metadata,
          m.created_at,
          m.updated_at
        from ai_models m
        left join ai_providers p
          on p.tenant_id = m.tenant_id
         and p.id = m.provider_id
        where m.tenant_id = $1
          and m.id = $2
        for update of m
        "#,
    )
    .bind(tenant_id)
    .bind(model_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai model not found"))
}

async fn update_model_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    model_id: Uuid,
    input: UpdateModelInput,
) -> Result<AiModel, AppError> {
    sqlx::query_as::<_, AiModel>(
        r#"
        update ai_models
        set
          name = $3,
          modality = $4,
          provider_id = $5,
          provider_model = $6,
          enabled = $7,
          currency = $8,
          billing_mode = $9,
          input_1k_price_minor = $10,
          output_1k_price_minor = $11,
          request_price_minor = $12,
          image_price_minor = $13,
          second_price_minor = $14,
          minute_price_minor = $15,
          daily_spend_limit_minor = $16,
          pricing_config_json = $17,
          metadata_json = $18,
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          code,
          name,
          modality,
          provider_id,
          null::text as provider_name,
          provider_model,
          enabled,
          currency,
          billing_mode,
          input_1k_price_minor,
          output_1k_price_minor,
          request_price_minor,
          image_price_minor,
          second_price_minor,
          minute_price_minor,
          daily_spend_limit_minor,
          pricing_config_json as pricing_config,
          metadata_json as metadata,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(model_id)
    .bind(input.name)
    .bind(input.modality)
    .bind(input.provider_id)
    .bind(input.provider_model)
    .bind(input.enabled)
    .bind(input.currency)
    .bind(input.billing_mode)
    .bind(input.input_1k_price_minor)
    .bind(input.output_1k_price_minor)
    .bind(input.request_price_minor)
    .bind(input.image_price_minor)
    .bind(input.second_price_minor)
    .bind(input.minute_price_minor)
    .bind(input.daily_spend_limit_minor)
    .bind(input.pricing_config)
    .bind(input.metadata)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn ensure_provider_exists_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    provider_id: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(provider_id) = provider_id else {
        return Ok(());
    };
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        select exists(
          select 1
          from ai_providers
          where tenant_id = $1
            and id = $2
        )
        "#,
    )
    .bind(tenant_id)
    .bind(provider_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found("ai provider not found"))
    }
}

async fn list_wallets(
    state: &AppState,
    tenant_id: Uuid,
    include_history: bool,
) -> Result<Vec<AiWalletSummary>, AppError> {
    sqlx::query_as::<_, AiWalletSummary>(
        r#"
        select
          c.id as customer_id,
          c.email as customer_email,
          c.name as customer_name,
          w.id as wallet_id,
          coalesce(w.currency, 'CNY') as currency,
          coalesce(w.balance_minor, 0) as balance_minor,
          coalesce(w.held_minor, 0) as held_minor,
          coalesce(w.balance_minor, 0) - coalesce(w.held_minor, 0) as available_minor,
          coalesce(w.ai_enabled, true) as ai_enabled,
          w.daily_spend_limit_minor,
          w.updated_at
        from customers c
        left join ai_wallets w
          on w.tenant_id = c.tenant_id
         and w.customer_id = c.id
        where c.tenant_id = $1
          and c.deleted_at is null
          and ($2::bool or c.status = 'active')
        order by c.created_at desc, c.id desc
        "#,
    )
    .bind(tenant_id)
    .bind(include_history)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn wallet_summary_by_customer(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<AiWalletSummary, AppError> {
    sqlx::query_as::<_, AiWalletSummary>(
        r#"
        select
          c.id as customer_id,
          c.email as customer_email,
          c.name as customer_name,
          w.id as wallet_id,
          coalesce(w.currency, 'CNY') as currency,
          coalesce(w.balance_minor, 0) as balance_minor,
          coalesce(w.held_minor, 0) as held_minor,
          coalesce(w.balance_minor, 0) - coalesce(w.held_minor, 0) as available_minor,
          coalesce(w.ai_enabled, true) as ai_enabled,
          w.daily_spend_limit_minor,
          w.updated_at
        from customers c
        left join ai_wallets w
          on w.tenant_id = c.tenant_id
         and w.customer_id = c.id
        where c.tenant_id = $1
          and c.id = $2
          and c.deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("customer not found"))
}

async fn ensure_customer_exists(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        select exists(
          select 1
          from customers
          where tenant_id = $1
            and id = $2
            and deleted_at is null
        )
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_one(&state.db)
    .await
    .map_err(map_db_error)?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found("customer not found"))
    }
}

async fn ensure_customer_exists_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        select exists(
          select 1
          from customers
          where tenant_id = $1
            and id = $2
            and deleted_at is null
        )
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)?;
    if exists {
        Ok(())
    } else {
        Err(AppError::not_found("customer not found"))
    }
}

async fn ensure_wallet_exists_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into ai_wallets (tenant_id, customer_id)
        values ($1, $2)
        on conflict (tenant_id, customer_id) do nothing
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(())
}

async fn find_wallet_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<AiWalletRecord, AppError> {
    sqlx::query_as::<_, AiWalletRecord>(
        r#"
        select
          id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor,
          updated_at
        from ai_wallets
        where tenant_id = $1
          and customer_id = $2
        for update
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai wallet not found"))
}

async fn update_wallet_balance_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    wallet_id: Uuid,
    amount_minor: i64,
) -> Result<AiWalletRecord, AppError> {
    let wallet = sqlx::query_as::<_, AiWalletRecord>(
        r#"
        select
          id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor,
          updated_at
        from ai_wallets
        where tenant_id = $1
          and id = $2
        for update
        "#,
    )
    .bind(tenant_id)
    .bind(wallet_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)?;
    let new_balance = wallet
        .balance_minor
        .checked_add(amount_minor)
        .ok_or_else(|| AppError::validation_failed("wallet balance adjustment overflow"))?;
    if new_balance < wallet.held_minor {
        return Err(AppError::business_rule_failed(
            "wallet balance cannot be lower than held amount",
        ));
    }

    sqlx::query_as::<_, AiWalletRecord>(
        r#"
        update ai_wallets
        set
          balance_minor = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(wallet_id)
    .bind(new_balance)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_wallet_quota_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    wallet_id: Uuid,
    daily_spend_limit_minor: Option<i64>,
) -> Result<AiWalletRecord, AppError> {
    sqlx::query_as::<_, AiWalletRecord>(
        r#"
        update ai_wallets
        set
          daily_spend_limit_minor = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(wallet_id)
    .bind(daily_spend_limit_minor)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_wallet_access_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    wallet_id: Uuid,
    ai_enabled: bool,
) -> Result<AiWalletRecord, AppError> {
    sqlx::query_as::<_, AiWalletRecord>(
        r#"
        update ai_wallets
        set
          ai_enabled = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
        returning
          id,
          customer_id,
          currency,
          balance_minor,
          held_minor,
          ai_enabled,
          daily_spend_limit_minor,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(wallet_id)
    .bind(ai_enabled)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn insert_wallet_ledger_entry(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    wallet: &AiWalletRecord,
    entry_type: &str,
    amount_minor: i64,
    reason: &str,
    metadata: Value,
    created_by: Uuid,
) -> Result<AiWalletLedgerEntry, AppError> {
    sqlx::query_as::<_, AiWalletLedgerEntry>(
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
          metadata_json,
          created_by
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        returning
          id,
          customer_id,
          null::text as customer_email,
          null::text as customer_name,
          $11::text as currency,
          entry_type,
          amount_minor,
          balance_after_minor,
          held_after_minor,
          reason,
          reference_type,
          reference_id,
          metadata_json as metadata,
          created_by,
          created_at
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
    .bind(metadata)
    .bind(created_by)
    .bind(&wallet.currency)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn list_wallet_ledger_entries(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
    page: i64,
    page_size: i64,
) -> Result<Vec<AiWalletLedgerEntry>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, AiWalletLedgerEntry>(
        r#"
        select
          l.id,
          l.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          w.currency,
          l.entry_type,
          l.amount_minor,
          l.balance_after_minor,
          l.held_after_minor,
          l.reason,
          l.reference_type,
          l.reference_id,
          l.metadata_json as metadata,
          l.created_by,
          l.created_at
        from ai_wallet_ledger_entries l
        join ai_wallets w
          on w.tenant_id = l.tenant_id
         and w.id = l.wallet_id
        join customers c
          on c.tenant_id = l.tenant_id
         and c.id = l.customer_id
        where l.tenant_id = $1
          and l.customer_id = $2
        order by l.created_at desc, l.id desc
        limit $3 offset $4
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn list_wallet_ledger_entries_for_tenant(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Option<Uuid>,
    entry_type: Option<&str>,
    reference_id: Option<&str>,
    page: i64,
    page_size: i64,
) -> Result<Vec<AiWalletLedgerEntry>, AppError> {
    let offset = (page - 1) * page_size;
    sqlx::query_as::<_, AiWalletLedgerEntry>(
        r#"
        select
          l.id,
          l.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          w.currency,
          l.entry_type,
          l.amount_minor,
          l.balance_after_minor,
          l.held_after_minor,
          l.reason,
          l.reference_type,
          l.reference_id,
          l.metadata_json as metadata,
          l.created_by,
          l.created_at
        from ai_wallet_ledger_entries l
        join ai_wallets w
          on w.tenant_id = l.tenant_id
         and w.id = l.wallet_id
        join customers c
          on c.tenant_id = l.tenant_id
         and c.id = l.customer_id
        where l.tenant_id = $1
          and ($2::uuid is null or l.customer_id = $2)
          and ($3::text is null or l.entry_type = $3)
          and ($4::text is null or l.reference_id = $4)
        order by l.created_at desc, l.id desc
        limit $5 offset $6
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(entry_type)
    .bind(reference_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

fn normalize_create_provider_input(
    state: &AppState,
    payload: CreateAiProviderRequest,
) -> Result<CreateProviderInput, AppError> {
    let name = normalize_name(&payload.name, "ai provider name", MAX_PROVIDER_NAME_LEN)?;
    let kind = normalize_provider_kind(&payload.kind)?;
    let base_url = normalize_base_url(&payload.base_url)?;
    let config = normalize_public_json(payload.config, "ai provider config", MAX_CONFIG_BYTES)?;
    let secret = payload
        .secret
        .map(|value| normalize_secret_json(value, "ai provider secret"))
        .transpose()?
        .filter(|value| !value.as_object().is_some_and(|map| map.is_empty()));
    let secret_encrypted = secret
        .as_ref()
        .map(|secret| encrypt_secret_to_text(state, secret))
        .transpose()?;

    Ok(CreateProviderInput {
        name,
        kind,
        base_url,
        enabled: payload.enabled.unwrap_or(true),
        config,
        secret_encrypted,
    })
}

fn normalize_update_provider_input(
    state: &AppState,
    before: &AiProviderRecord,
    payload: UpdateAiProviderRequest,
) -> Result<UpdateProviderInput, AppError> {
    let name = match payload.name {
        Some(name) => normalize_name(&name, "ai provider name", MAX_PROVIDER_NAME_LEN)?,
        None => before.name.clone(),
    };
    let base_url = match payload.base_url {
        Some(base_url) => normalize_base_url(&base_url)?,
        None => before.base_url.clone(),
    };
    let config = match payload.config {
        Some(config) => normalize_public_json(config, "ai provider config", MAX_CONFIG_BYTES)?,
        None => before.config.clone(),
    };
    let new_secret = payload
        .secret
        .map(|value| normalize_secret_json(value, "ai provider secret"))
        .transpose()?
        .filter(|value| !value.as_object().is_some_and(|map| map.is_empty()));
    let secret_encrypted = match (payload.clear_secret, new_secret) {
        (true, _) => None,
        (false, Some(secret)) => Some(encrypt_secret_to_text(state, &secret)?),
        (false, None) => before.secret_encrypted.clone(),
    };

    Ok(UpdateProviderInput {
        name,
        base_url,
        enabled: payload.enabled.unwrap_or(before.enabled),
        config,
        secret_encrypted,
    })
}

fn normalize_create_model_input(
    payload: CreateAiModelRequest,
) -> Result<CreateModelInput, AppError> {
    let modality = normalize_modality(&payload.modality)?;
    let input_1k_price_minor =
        normalize_nonnegative_price(payload.input_1k_price_minor.unwrap_or(0))?;
    let output_1k_price_minor =
        normalize_nonnegative_price(payload.output_1k_price_minor.unwrap_or(0))?;
    let request_price_minor =
        normalize_nonnegative_price(payload.request_price_minor.unwrap_or(0))?;
    let image_price_minor = normalize_nonnegative_price(payload.image_price_minor.unwrap_or(0))?;
    let second_price_minor = normalize_nonnegative_price(payload.second_price_minor.unwrap_or(0))?;
    let minute_price_minor = normalize_nonnegative_price(payload.minute_price_minor.unwrap_or(0))?;
    let billing_mode = match payload.billing_mode.as_deref() {
        Some(value) => normalize_billing_mode(value, &modality)?,
        None => default_billing_mode(&modality, request_price_minor, second_price_minor),
    };

    Ok(CreateModelInput {
        code: normalize_model_code(&payload.code)?,
        name: normalize_name(&payload.name, "ai model name", MAX_MODEL_NAME_LEN)?,
        modality,
        provider_id: payload.provider_id,
        provider_model: payload
            .provider_model
            .as_deref()
            .map(normalize_provider_model)
            .transpose()?,
        enabled: payload.enabled.unwrap_or(true),
        currency: normalize_currency(payload.currency.as_deref().unwrap_or("CNY"))?,
        billing_mode,
        input_1k_price_minor,
        output_1k_price_minor,
        request_price_minor,
        image_price_minor,
        second_price_minor,
        minute_price_minor,
        daily_spend_limit_minor: normalize_optional_limit(
            payload.daily_spend_limit_minor,
            "ai model daily spend limit",
        )?,
        pricing_config: normalize_model_pricing_config(payload.pricing_config)?,
        metadata: normalize_public_json(payload.metadata, "ai model metadata", MAX_CONFIG_BYTES)?,
    })
}

fn normalize_update_model_input(
    before: &AiModel,
    payload: UpdateAiModelRequest,
) -> Result<UpdateModelInput, AppError> {
    let modality_changed = payload.modality.is_some();
    let modality = match payload.modality {
        Some(modality) => normalize_modality(&modality)?,
        None => before.modality.clone(),
    };
    let input_1k_price_minor = normalize_nonnegative_price(
        payload
            .input_1k_price_minor
            .unwrap_or(before.input_1k_price_minor),
    )?;
    let output_1k_price_minor = normalize_nonnegative_price(
        payload
            .output_1k_price_minor
            .unwrap_or(before.output_1k_price_minor),
    )?;
    let request_price_minor = normalize_nonnegative_price(
        payload
            .request_price_minor
            .unwrap_or(before.request_price_minor),
    )?;
    let image_price_minor = normalize_nonnegative_price(
        payload
            .image_price_minor
            .unwrap_or(before.image_price_minor),
    )?;
    let second_price_minor = normalize_nonnegative_price(
        payload
            .second_price_minor
            .unwrap_or(before.second_price_minor),
    )?;
    let minute_price_minor = normalize_nonnegative_price(
        payload
            .minute_price_minor
            .unwrap_or(before.minute_price_minor),
    )?;
    let billing_mode = match payload.billing_mode {
        Some(billing_mode) => normalize_billing_mode(&billing_mode, &modality)?,
        None if modality_changed && modality != before.modality => {
            default_billing_mode(&modality, request_price_minor, second_price_minor)
        }
        None => normalize_billing_mode(&before.billing_mode, &modality)?,
    };

    Ok(UpdateModelInput {
        name: match payload.name {
            Some(name) => normalize_name(&name, "ai model name", MAX_MODEL_NAME_LEN)?,
            None => before.name.clone(),
        },
        modality,
        provider_id: payload.provider_id.unwrap_or(before.provider_id),
        provider_model: match payload.provider_model {
            Some(Some(provider_model)) => Some(normalize_provider_model(&provider_model)?),
            Some(None) => None,
            None => before.provider_model.clone(),
        },
        enabled: payload.enabled.unwrap_or(before.enabled),
        currency: match payload.currency {
            Some(currency) => normalize_currency(&currency)?,
            None => before.currency.clone(),
        },
        billing_mode,
        input_1k_price_minor,
        output_1k_price_minor,
        request_price_minor,
        image_price_minor,
        second_price_minor,
        minute_price_minor,
        daily_spend_limit_minor: match payload.daily_spend_limit_minor {
            Some(value) => normalize_optional_limit(value, "ai model daily spend limit")?,
            None => before.daily_spend_limit_minor,
        },
        pricing_config: match payload.pricing_config {
            Some(pricing_config) => normalize_model_pricing_config(pricing_config)?,
            None => before.pricing_config.clone(),
        },
        metadata: match payload.metadata {
            Some(metadata) => {
                normalize_public_json(metadata, "ai model metadata", MAX_CONFIG_BYTES)?
            }
            None => before.metadata.clone(),
        },
    })
}

fn normalize_name(value: &str, label: &str, max_len: usize) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_len || value.contains('\0') {
        return Err(AppError::validation_failed(format!("{label} is invalid")));
    }

    Ok(value.to_owned())
}

fn normalize_provider_kind(kind: &str) -> Result<String, AppError> {
    let kind = kind.trim().to_ascii_lowercase().replace('-', "_");
    match kind.as_str() {
        "openai_compatible" | "custom_http" | "claude" | "gemini" | "deepseek" | "image"
        | "video" | "wuyin_keji" => Ok(kind),
        _ => Err(AppError::validation_failed("ai provider kind is invalid")),
    }
}

fn normalize_base_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    let parsed = reqwest::Url::parse(value)
        .map_err(|_| AppError::validation_failed("ai provider base_url is invalid"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppError::validation_failed(
            "ai provider base_url must be http or https",
        ));
    }

    Ok(value.trim_end_matches('/').to_owned())
}

fn normalize_model_code(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > MAX_MODEL_CODE_LEN
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/'))
    {
        return Err(AppError::validation_failed("ai model code is invalid"));
    }

    Ok(value)
}

fn normalize_modality(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "text" | "image" | "video" | "audio" | "embedding" | "multimodal" => Ok(value),
        _ => Err(AppError::validation_failed("ai model modality is invalid")),
    }
}

fn normalize_billing_mode(value: &str, modality: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    let valid = match modality {
        "text" | "embedding" => value == "token",
        "image" => value == "per_image",
        "video" => matches!(value.as_str(), "video_per_second" | "video_per_request"),
        "audio" => matches!(
            value.as_str(),
            "audio_per_second" | "audio_per_minute" | "audio_per_request"
        ),
        "multimodal" => matches!(value.as_str(), "token" | "per_image"),
        _ => false,
    };
    if valid {
        Ok(value)
    } else {
        Err(AppError::validation_failed(
            "ai model billing mode is invalid for modality",
        ))
    }
}

fn default_billing_mode(
    modality: &str,
    request_price_minor: i64,
    second_price_minor: i64,
) -> String {
    match modality {
        "image" => "per_image",
        "video" if request_price_minor > 0 && second_price_minor == 0 => "video_per_request",
        "video" => "video_per_second",
        "audio" if request_price_minor > 0 && second_price_minor == 0 => "audio_per_request",
        "audio" => "audio_per_second",
        _ => "token",
    }
    .to_owned()
}

fn normalize_provider_model(value: &str) -> Result<String, AppError> {
    normalize_name(value, "ai provider model", MAX_PROVIDER_MODEL_LEN)
}

fn normalize_currency(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() == 3 && value.chars().all(|ch| ch.is_ascii_uppercase()) {
        Ok(value)
    } else {
        Err(AppError::validation_failed(
            "ai billing currency is invalid",
        ))
    }
}

fn normalize_nonnegative_price(value: i64) -> Result<i64, AppError> {
    if value < 0 {
        return Err(AppError::validation_failed(
            "ai model price cannot be negative",
        ));
    }

    Ok(value)
}

fn normalize_model_pricing_config(value: Value) -> Result<Value, AppError> {
    let value = normalize_public_json(value, "ai model pricing config", MAX_CONFIG_BYTES)?;
    validate_capabilities_config(&value)?;

    Ok(value)
}

fn normalize_optional_limit(value: Option<i64>, label: &str) -> Result<Option<i64>, AppError> {
    validate_optional_limit(value, label)?;

    Ok(value)
}

fn validate_optional_limit(value: Option<i64>, label: &str) -> Result<(), AppError> {
    if value.is_some_and(|value| value < 0) {
        return Err(AppError::validation_failed(format!(
            "{label} must be greater than or equal to 0"
        )));
    }

    Ok(())
}

fn normalize_adjustment_amount(amount_minor: i64) -> Result<i64, AppError> {
    if amount_minor == 0 || amount_minor.abs() > MAX_MANUAL_ADJUSTMENT_MINOR {
        return Err(AppError::validation_failed(
            "ai wallet adjustment amount is invalid",
        ));
    }

    Ok(amount_minor)
}

fn normalize_reason(value: &str) -> Result<String, AppError> {
    normalize_name(value, "ai wallet adjustment reason", MAX_REASON_LEN)
}

fn normalize_ledger_entry_type(value: &str) -> Result<String, AppError> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "credit" | "debit" | "hold" | "capture" | "release" | "refund" | "adjustment" => Ok(value),
        _ => Err(AppError::validation_failed(
            "ai wallet ledger entry type is invalid",
        )),
    }
}

fn clean_optional_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_public_json(value: Value, label: &str, max_bytes: usize) -> Result<Value, AppError> {
    let value = if value.is_null() { json!({}) } else { value };
    ensure_json_object(label, &value)?;
    reject_sensitive_public_keys(&value)?;
    ensure_json_size(label, &value, max_bytes)?;

    Ok(value)
}

fn normalize_metadata(value: Value) -> Result<Value, AppError> {
    normalize_public_json(value, "ai wallet metadata", MAX_CONFIG_BYTES)
}

fn normalize_secret_json(value: Value, label: &str) -> Result<Value, AppError> {
    let value = if value.is_null() { json!({}) } else { value };
    ensure_json_object(label, &value)?;
    ensure_json_size(label, &value, MAX_SECRET_BYTES)?;

    Ok(value)
}

fn ensure_json_object(label: &str, value: &Value) -> Result<(), AppError> {
    if value.is_object() {
        return Ok(());
    }

    Err(AppError::validation_failed(format!(
        "{label} must be an object"
    )))
}

fn ensure_json_size(label: &str, value: &Value, max_bytes: usize) -> Result<(), AppError> {
    let len = serde_json::to_vec(value)
        .map_err(|error| AppError::validation_failed(format!("{label} invalid: {error}")))?
        .len();
    if len > max_bytes {
        return Err(AppError::validation_failed(format!("{label} is too large")));
    }

    Ok(())
}

fn reject_sensitive_public_keys(value: &Value) -> Result<(), AppError> {
    const SENSITIVE_PARTS: &[&str] = &[
        "secret",
        "password",
        "token",
        "api_key",
        "apikey",
        "authorization",
        "credential",
        "credentials",
        "private",
    ];

    fn visit(value: &Value, sensitive_parts: &[&str]) -> bool {
        match value {
            Value::Object(map) => map.iter().any(|(key, value)| {
                let normalized = key.to_ascii_lowercase();
                sensitive_parts.iter().any(|part| normalized.contains(part))
                    || visit(value, sensitive_parts)
            }),
            Value::Array(items) => items.iter().any(|item| visit(item, sensitive_parts)),
            _ => false,
        }
    }

    if visit(value, SENSITIVE_PARTS) {
        return Err(AppError::validation_failed(
            "sensitive ai provider values must be stored as secret",
        ));
    }

    Ok(())
}

fn normalize_page(page: Option<i64>, page_size: Option<i64>) -> (i64, i64) {
    let page = page.unwrap_or(1).max(1);
    let page_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);

    (page, page_size)
}

fn encrypt_secret_to_text(state: &AppState, secret: &Value) -> Result<String, AppError> {
    let plaintext = serde_json::to_vec(secret)
        .map_err(|error| AppError::crypto(format!("ai provider secret invalid: {error}")))?;
    let envelope = encrypt_bytes(&state.config.security.master_key, &plaintext)?;

    serde_json::to_string(&envelope).map_err(|error| {
        AppError::crypto(format!(
            "ai provider secret envelope serialization failed: {error}"
        ))
    })
}

#[allow(dead_code)]
fn decrypt_secret_text(state: &AppState, encrypted_secret: &str) -> Result<Value, AppError> {
    let envelope: PrivateKeyEnvelope = serde_json::from_str(encrypted_secret).map_err(|error| {
        AppError::crypto(format!("ai provider secret envelope invalid: {error}"))
    })?;
    let plaintext = decrypt_bytes(&state.config.security.master_key, &envelope)?;

    serde_json::from_slice(&plaintext)
        .map_err(|error| AppError::crypto(format!("ai provider secret plaintext invalid: {error}")))
}

fn provider_audit_json(record: &AiProviderRecord) -> Value {
    json!({
        "id": record.id,
        "name": &record.name,
        "kind": &record.kind,
        "base_url": &record.base_url,
        "enabled": record.enabled,
        "config": &record.config,
        "secret_configured": record.secret_encrypted.is_some(),
        "created_at": &record.created_at,
        "updated_at": &record.updated_at,
    })
}

fn model_audit_json(record: &AiModel) -> Value {
    json!({
        "id": record.id,
        "code": &record.code,
        "name": &record.name,
        "modality": &record.modality,
        "provider_id": record.provider_id,
        "provider_model": &record.provider_model,
        "enabled": record.enabled,
        "currency": &record.currency,
        "billing_mode": &record.billing_mode,
        "input_1k_price_minor": record.input_1k_price_minor,
        "output_1k_price_minor": record.output_1k_price_minor,
        "request_price_minor": record.request_price_minor,
        "image_price_minor": record.image_price_minor,
        "second_price_minor": record.second_price_minor,
        "minute_price_minor": record.minute_price_minor,
        "daily_spend_limit_minor": record.daily_spend_limit_minor,
        "pricing_config": &record.pricing_config,
        "metadata": &record.metadata,
        "created_at": &record.created_at,
        "updated_at": &record.updated_at,
    })
}

fn wallet_audit_json(record: &AiWalletRecord) -> Value {
    json!({
        "id": record.id,
        "customer_id": record.customer_id,
        "currency": &record.currency,
        "balance_minor": record.balance_minor,
        "held_minor": record.held_minor,
        "ai_enabled": record.ai_enabled,
        "daily_spend_limit_minor": record.daily_spend_limit_minor,
        "updated_at": &record.updated_at,
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
    AppError::dependency(format!("ai admin database error: {error}"))
}

impl From<AiProviderRecord> for AiProvider {
    fn from(record: AiProviderRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            kind: record.kind,
            base_url: record.base_url,
            enabled: record.enabled,
            config: record.config,
            secret_configured: record.secret_encrypted.is_some(),
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use crate::modules::auth::session::AdminContext;

    use super::{
        default_billing_mode, ensure_admin_permission, normalize_adjustment_amount,
        normalize_base_url, normalize_billing_mode, normalize_modality, normalize_model_code,
        normalize_provider_kind, normalize_public_json,
    };

    #[test]
    fn provider_kind_and_base_url_are_validated() {
        assert_eq!(
            normalize_provider_kind("openai-compatible").expect("kind"),
            "openai_compatible"
        );
        assert_eq!(
            normalize_provider_kind("wuyin-keji").expect("kind"),
            "wuyin_keji"
        );
        assert!(normalize_provider_kind("random").is_err());
        assert_eq!(
            normalize_base_url("https://api.example.com/v1/").expect("url"),
            "https://api.example.com/v1"
        );
        assert!(normalize_base_url("file:///tmp/provider").is_err());
    }

    #[test]
    fn model_code_and_modality_are_validated() {
        assert_eq!(
            normalize_model_code(" GPT-4O-MINI ").expect("code"),
            "gpt-4o-mini"
        );
        assert!(normalize_model_code("bad code").is_err());
        assert_eq!(normalize_modality("Video").expect("modality"), "video");
        assert!(normalize_modality("document").is_err());
    }

    #[test]
    fn billing_mode_is_validated_by_modality() {
        assert_eq!(
            normalize_billing_mode("token", "text").expect("billing mode"),
            "token"
        );
        assert_eq!(
            normalize_billing_mode("per_image", "image").expect("billing mode"),
            "per_image"
        );
        assert_eq!(
            normalize_billing_mode("audio_per_minute", "audio").expect("billing mode"),
            "audio_per_minute"
        );
        assert!(normalize_billing_mode("per_image", "text").is_err());
        assert!(normalize_billing_mode("audio_per_minute", "video").is_err());
    }

    #[test]
    fn default_billing_mode_keeps_legacy_prices_reasonable() {
        assert_eq!(default_billing_mode("text", 0, 0), "token");
        assert_eq!(default_billing_mode("image", 0, 0), "per_image");
        assert_eq!(default_billing_mode("video", 300, 0), "video_per_request");
        assert_eq!(default_billing_mode("video", 0, 25), "video_per_second");
    }

    #[test]
    fn public_config_rejects_sensitive_keys() {
        assert!(normalize_public_json(json!({"timeout_ms": 30000}), "config", 1024).is_ok());
        assert!(normalize_public_json(json!({"api_key": "secret"}), "config", 1024).is_err());
        assert!(normalize_public_json(
            json!({"headers": {"Authorization": "Bearer x"}}),
            "config",
            1024
        )
        .is_err());
    }

    #[test]
    fn manual_adjustment_amount_is_bounded() {
        assert_eq!(normalize_adjustment_amount(100).expect("amount"), 100);
        assert_eq!(normalize_adjustment_amount(-100).expect("amount"), -100);
        assert!(normalize_adjustment_amount(0).is_err());
    }

    #[test]
    fn permission_check_uses_ai_permissions() {
        let mut admin = AdminContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            team_member_id: Uuid::nil(),
            email: "admin@example.com".to_owned(),
            name: "Admin".to_owned(),
            email_verified: true,
            mfa_enabled: false,
            tenant_name: "Default".to_owned(),
            roles: vec!["admin".to_owned()],
            permissions: vec!["ai:read".to_owned()],
        };

        assert!(ensure_admin_permission(&admin, "ai:read").is_ok());
        assert!(ensure_admin_permission(&admin, "ai:wallet:update").is_err());
        admin.permissions.push("ai:wallet:update".to_owned());
        assert!(ensure_admin_permission(&admin, "ai:wallet:update").is_ok());
    }
}
