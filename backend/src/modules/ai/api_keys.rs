use axum::{
    extract::{Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

const AI_API_KEY_PREFIX: &str = "ehai_";
const DISPLAY_PREFIX_LEN: usize = 18;
const MAX_KEY_NAME_LEN: usize = 128;

#[derive(Debug, Clone)]
pub struct AiApiKeyContext {
    pub api_key_id: Uuid,
    pub tenant_id: Uuid,
    pub customer_id: Uuid,
    pub daily_spend_limit_minor: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AiApiKey {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub customer_email: String,
    pub customer_name: Option<String>,
    pub name: String,
    pub key_prefix: String,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub daily_spend_limit_minor: Option<i64>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct AiApiKeyListResponse {
    pub items: Vec<AiApiKey>,
}

#[derive(Debug, Serialize)]
pub struct CreateAiApiKeyResponse {
    pub api_key: AiApiKey,
    pub plain_key: String,
}

#[derive(Debug, Serialize)]
pub struct AiApiKeyResponse {
    pub api_key: AiApiKey,
}

#[derive(Debug, Deserialize)]
pub struct AiApiKeyListQuery {
    pub include_history: Option<bool>,
    pub customer_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CreateAiApiKeyRequest {
    pub name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub daily_spend_limit_minor: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAiApiKeyRequest {
    pub name: Option<String>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub daily_spend_limit_minor: Option<Option<i64>>,
}

#[derive(Debug, FromRow)]
struct ApiKeyAuthRecord {
    id: Uuid,
    tenant_id: Uuid,
    customer_id: Uuid,
    daily_spend_limit_minor: Option<i64>,
}

pub async fn list_ai_api_keys(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<AiApiKeyListQuery>,
) -> Result<Json<ApiResponse<AiApiKeyListResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:read")?;

    let items = list_api_keys(
        &state,
        admin.tenant_id,
        query.customer_id,
        query.include_history.unwrap_or(false),
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        AiApiKeyListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn create_ai_api_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(customer_id): Path<Uuid>,
    Json(payload): Json<CreateAiApiKeyRequest>,
) -> Result<Json<ApiResponse<CreateAiApiKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:api_key:update")?;
    let name = normalize_key_name(&payload.name)?;
    validate_optional_limit(
        payload.daily_spend_limit_minor,
        "ai api key daily spend limit",
    )?;
    ensure_active_customer(&state, admin.tenant_id, customer_id).await?;

    let plain_key = generate_ai_api_key();
    let key_prefix = display_prefix(&plain_key);
    let key_hash = hash_token(&state.config.security.token_hash_pepper, &plain_key)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        insert into ai_api_keys (
          id,
          tenant_id,
          customer_id,
          name,
          key_prefix,
          key_hash,
          expires_at,
          daily_spend_limit_minor,
          created_by
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(id)
    .bind(admin.tenant_id)
    .bind(customer_id)
    .bind(&name)
    .bind(&key_prefix)
    .bind(key_hash)
    .bind(payload.expires_at)
    .bind(payload.daily_spend_limit_minor)
    .bind(admin.team_member_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_api_key.create",
            resource_type: "ai_api_key",
            resource_id: Some(id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(json!({
                "id": id,
                "customer_id": customer_id,
                "name": &name,
                "key_prefix": &key_prefix,
                "expires_at": payload.expires_at,
                "daily_spend_limit_minor": payload.daily_spend_limit_minor,
            })),
            metadata_json: json!({
                "customer_id": customer_id,
                "key_prefix": &key_prefix,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let api_key = find_api_key(&state, admin.tenant_id, id).await?;

    Ok(Json(ApiResponse::ok(
        CreateAiApiKeyResponse { api_key, plain_key },
        request_id.to_string(),
    )))
}

pub async fn revoke_ai_api_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(api_key_id): Path<Uuid>,
) -> Result<Json<ApiResponse<AiApiKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:api_key:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_api_key_for_update(&mut transaction, admin.tenant_id, api_key_id).await?;
    if before.status == "revoked" {
        return Err(AppError::already_revoked("ai api key already revoked"));
    }

    sqlx::query(
        r#"
        update ai_api_keys
        set status = 'revoked',
            revoked_at = now()
        where tenant_id = $1
          and id = $2
        "#,
    )
    .bind(admin.tenant_id)
    .bind(api_key_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_api_key.revoke",
            resource_type: "ai_api_key",
            resource_id: Some(api_key_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "id": before.id,
                "customer_id": before.customer_id,
                "name": &before.name,
                "key_prefix": &before.key_prefix,
                "status": &before.status,
                "expires_at": before.expires_at,
                "daily_spend_limit_minor": before.daily_spend_limit_minor,
            })),
            after_json: Some(json!({
                "id": before.id,
                "customer_id": before.customer_id,
                "name": &before.name,
                "key_prefix": &before.key_prefix,
                "status": "revoked",
                "expires_at": before.expires_at,
                "daily_spend_limit_minor": before.daily_spend_limit_minor,
            })),
            metadata_json: json!({
                "customer_id": before.customer_id,
                "key_prefix": &before.key_prefix,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let api_key = find_api_key(&state, admin.tenant_id, api_key_id).await?;

    Ok(Json(ApiResponse::ok(
        AiApiKeyResponse { api_key },
        request_id.to_string(),
    )))
}

pub async fn update_ai_api_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(api_key_id): Path<Uuid>,
    Json(payload): Json<UpdateAiApiKeyRequest>,
) -> Result<Json<ApiResponse<AiApiKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "ai:api_key:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_api_key_for_update(&mut transaction, admin.tenant_id, api_key_id).await?;
    let name = match payload.name {
        Some(name) => normalize_key_name(&name)?,
        None => before.name.clone(),
    };
    let expires_at = payload.expires_at.unwrap_or(before.expires_at);
    let daily_spend_limit_minor = match payload.daily_spend_limit_minor {
        Some(limit) => {
            validate_optional_limit(limit, "ai api key daily spend limit")?;
            limit
        }
        None => before.daily_spend_limit_minor,
    };

    sqlx::query(
        r#"
        update ai_api_keys
        set name = $3,
            expires_at = $4,
            daily_spend_limit_minor = $5
        where tenant_id = $1
          and id = $2
        "#,
    )
    .bind(admin.tenant_id)
    .bind(api_key_id)
    .bind(&name)
    .bind(expires_at)
    .bind(daily_spend_limit_minor)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "ai_api_key.update",
            resource_type: "ai_api_key",
            resource_id: Some(api_key_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(api_key_audit_json(&before)),
            after_json: Some(json!({
                "id": api_key_id,
                "customer_id": before.customer_id,
                "name": &name,
                "key_prefix": &before.key_prefix,
                "status": &before.status,
                "expires_at": expires_at,
                "daily_spend_limit_minor": daily_spend_limit_minor,
            })),
            metadata_json: json!({
                "customer_id": before.customer_id,
                "key_prefix": &before.key_prefix,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let api_key = find_api_key(&state, admin.tenant_id, api_key_id).await?;

    Ok(Json(ApiResponse::ok(
        AiApiKeyResponse { api_key },
        request_id.to_string(),
    )))
}

pub async fn authenticate_api_key(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AiApiKeyContext, AppError> {
    let token = bearer_token(headers)?;
    if !token.starts_with(AI_API_KEY_PREFIX) {
        return Err(AppError::invalid_credentials());
    }
    let key_hash = hash_token(&state.config.security.token_hash_pepper, token)?;

    let record = sqlx::query_as::<_, ApiKeyAuthRecord>(
        r#"
        select
          k.id,
          k.tenant_id,
          k.customer_id,
          k.daily_spend_limit_minor
        from ai_api_keys k
        join tenants t
          on t.id = k.tenant_id
        join customers c
          on c.id = k.customer_id
          and c.tenant_id = k.tenant_id
        where k.key_hash = $1
          and k.status = 'active'
          and k.revoked_at is null
          and (k.expires_at is null or k.expires_at > now())
          and t.status = 'active'
          and c.deleted_at is null
          and c.status = 'active'
        "#,
    )
    .bind(key_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::invalid_credentials)?;

    sqlx::query(
        r#"
        update ai_api_keys
        set last_used_at = now()
        where id = $1
        "#,
    )
    .bind(record.id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(AiApiKeyContext {
        api_key_id: record.id,
        tenant_id: record.tenant_id,
        customer_id: record.customer_id,
        daily_spend_limit_minor: record.daily_spend_limit_minor,
    })
}

async fn list_api_keys(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Option<Uuid>,
    include_history: bool,
) -> Result<Vec<AiApiKey>, AppError> {
    sqlx::query_as::<_, AiApiKey>(
        r#"
        select
          k.id,
          k.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          k.name,
          k.key_prefix,
          k.status,
          k.expires_at,
          k.daily_spend_limit_minor,
          k.last_used_at,
          k.created_at,
          k.revoked_at
        from ai_api_keys k
        join customers c
          on c.id = k.customer_id
          and c.tenant_id = k.tenant_id
        where k.tenant_id = $1
          and ($2::uuid is null or k.customer_id = $2)
          and ($3::bool or k.status = 'active')
        order by k.created_at desc, k.id desc
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .bind(include_history)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_api_key(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<AiApiKey, AppError> {
    sqlx::query_as::<_, AiApiKey>(
        r#"
        select
          k.id,
          k.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          k.name,
          k.key_prefix,
          k.status,
          k.expires_at,
          k.daily_spend_limit_minor,
          k.last_used_at,
          k.created_at,
          k.revoked_at
        from ai_api_keys k
        join customers c
          on c.id = k.customer_id
          and c.tenant_id = k.tenant_id
        where k.tenant_id = $1
          and k.id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai api key not found"))
}

async fn find_api_key_for_update(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<AiApiKey, AppError> {
    sqlx::query_as::<_, AiApiKey>(
        r#"
        select
          k.id,
          k.customer_id,
          c.email as customer_email,
          c.name as customer_name,
          k.name,
          k.key_prefix,
          k.status,
          k.expires_at,
          k.daily_spend_limit_minor,
          k.last_used_at,
          k.created_at,
          k.revoked_at
        from ai_api_keys k
        join customers c
          on c.id = k.customer_id
          and c.tenant_id = k.tenant_id
        where k.tenant_id = $1
          and k.id = $2
        for update of k
        "#,
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("ai api key not found"))
}

async fn ensure_active_customer(
    state: &AppState,
    tenant_id: Uuid,
    customer_id: Uuid,
) -> Result<(), AppError> {
    let status = sqlx::query_scalar::<_, String>(
        r#"
        select status
        from customers
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(customer_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("customer not found"))?;

    if status != "active" {
        return Err(AppError::business_rule_failed(
            "ai api key can only be created for active customers",
        ));
    }

    Ok(())
}

fn normalize_key_name(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_KEY_NAME_LEN || value.contains('\0') {
        return Err(AppError::validation_failed("ai api key name is invalid"));
    }

    Ok(value.to_owned())
}

fn validate_optional_limit(value: Option<i64>, field: &str) -> Result<(), AppError> {
    if value.is_some_and(|value| value < 0) {
        return Err(AppError::validation_failed(format!(
            "{field} must be greater than or equal to 0"
        )));
    }

    Ok(())
}

fn generate_ai_api_key() -> String {
    format!("{AI_API_KEY_PREFIX}{}", generate_token())
}

fn display_prefix(key: &str) -> String {
    key.chars().take(DISPLAY_PREFIX_LEN).collect()
}

fn api_key_audit_json(record: &AiApiKey) -> serde_json::Value {
    json!({
        "id": record.id,
        "customer_id": record.customer_id,
        "name": &record.name,
        "key_prefix": &record.key_prefix,
        "status": &record.status,
        "expires_at": record.expires_at,
        "daily_spend_limit_minor": record.daily_spend_limit_minor,
        "last_used_at": record.last_used_at,
        "created_at": record.created_at,
        "revoked_at": record.revoked_at,
    })
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, AppError> {
    let value = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(AppError::unauthenticated)?;
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(AppError::unauthenticated());
    };
    if token.trim().is_empty() {
        return Err(AppError::unauthenticated());
    }

    Ok(token)
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
    AppError::dependency(format!("ai api key database error: {error}"))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{bearer_token, display_prefix, generate_ai_api_key, normalize_key_name};

    #[test]
    fn generated_key_has_public_prefix() {
        let key = generate_ai_api_key();

        assert!(key.starts_with("ehai_"));
        assert!(key.len() > 40);
        assert_eq!(display_prefix(&key).chars().count(), 18);
    }

    #[test]
    fn key_name_is_validated() {
        assert_eq!(normalize_key_name(" SDK ").expect("name"), "SDK");
        assert!(normalize_key_name("").is_err());
    }

    #[test]
    fn bearer_token_reads_ai_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ehai_test"),
        );

        assert_eq!(bearer_token(&headers).expect("bearer"), "ehai_test");
    }
}
