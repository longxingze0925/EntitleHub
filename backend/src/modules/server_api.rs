use axum::{
    extract::{Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        subscription::repository::SubscriptionRepository,
    },
    state::AppState,
};

const SERVER_API_KEY_PREFIX: &str = "ehsk_";
const DISPLAY_PREFIX_LEN: usize = 18;
const MAX_KEY_NAME_LEN: usize = 128;
const CUSTOMER_ID_HEADER: &str = "x-entitlehub-customer-id";
const SCOPE_AI_INVOKE: &str = concat!("ai", ":", "invoke");
const ALLOWED_SCOPES: &[&str] = &[SCOPE_AI_INVOKE];

#[derive(Debug, Clone)]
pub struct ServerApiKeyContext {
    pub server_key_id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ServerApiKey {
    pub id: Uuid,
    pub app_id: Uuid,
    pub app_name: String,
    pub app_key: String,
    pub name: String,
    pub key_prefix: String,
    pub status: String,
    pub scopes: Value,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct ServerApiKeyListResponse {
    pub items: Vec<ServerApiKey>,
}

#[derive(Debug, Serialize)]
pub struct CreateServerApiKeyResponse {
    pub server_api_key: ServerApiKey,
    pub plain_key: String,
}

#[derive(Debug, Serialize)]
pub struct ServerApiKeyResponse {
    pub server_api_key: ServerApiKey,
}

#[derive(Debug, Deserialize)]
pub struct ServerApiKeyListQuery {
    pub include_history: Option<bool>,
    pub app_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct CreateServerApiKeyRequest {
    pub app_id: Uuid,
    pub name: String,
    pub scopes: Option<Vec<String>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServerApiKeyRequest {
    pub name: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, FromRow)]
struct ServerApiKeyAuthRecord {
    id: Uuid,
    tenant_id: Uuid,
    app_id: Uuid,
    scopes: Value,
}

pub async fn list_server_api_keys(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<ServerApiKeyListQuery>,
) -> Result<Json<ApiResponse<ServerApiKeyListResponse>>, AppError> {
    ensure_admin_permission(&admin, "server_api_key:read")?;

    let items = list_keys(
        &state,
        admin.tenant_id,
        query.app_id,
        query.include_history.unwrap_or(false),
    )
    .await?;

    Ok(Json(ApiResponse::ok(
        ServerApiKeyListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn create_server_api_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateServerApiKeyRequest>,
) -> Result<Json<ApiResponse<CreateServerApiKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "server_api_key:update")?;
    let name = normalize_key_name(&payload.name)?;
    let scopes = normalize_scopes(payload.scopes)?;
    ensure_active_application(&state, admin.tenant_id, payload.app_id).await?;

    let plain_key = generate_server_api_key();
    let key_prefix = display_prefix(&plain_key);
    let key_hash = hash_token(&state.config.security.token_hash_pepper, &plain_key)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        insert into server_api_keys (
          id,
          tenant_id,
          app_id,
          name,
          key_prefix,
          key_hash,
          scopes,
          expires_at,
          created_by
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind(id)
    .bind(admin.tenant_id)
    .bind(payload.app_id)
    .bind(&name)
    .bind(&key_prefix)
    .bind(key_hash)
    .bind(json!(scopes))
    .bind(payload.expires_at)
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
            action: "server_api_key.create",
            resource_type: "server_api_key",
            resource_id: Some(id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(json!({
                "id": id,
                "app_id": payload.app_id,
                "name": &name,
                "key_prefix": &key_prefix,
                "scopes": scopes,
                "expires_at": payload.expires_at,
            })),
            metadata_json: json!({
                "app_id": payload.app_id,
                "key_prefix": &key_prefix,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let server_api_key = find_key(&state, admin.tenant_id, id).await?;

    Ok(Json(ApiResponse::ok(
        CreateServerApiKeyResponse {
            server_api_key,
            plain_key,
        },
        request_id.to_string(),
    )))
}

pub async fn update_server_api_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(server_key_id): Path<Uuid>,
    Json(payload): Json<UpdateServerApiKeyRequest>,
) -> Result<Json<ApiResponse<ServerApiKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "server_api_key:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_key_for_update(&mut transaction, admin.tenant_id, server_key_id).await?;
    let name = match payload.name {
        Some(name) => normalize_key_name(&name)?,
        None => before.name.clone(),
    };
    let scopes = match payload.scopes {
        Some(scopes) => normalize_scopes(Some(scopes))?,
        None => scopes_from_value(&before.scopes)?,
    };
    let expires_at = payload.expires_at.unwrap_or(before.expires_at);

    sqlx::query(
        r#"
        update server_api_keys
        set name = $3,
            scopes = $4,
            expires_at = $5
        where tenant_id = $1
          and id = $2
        "#,
    )
    .bind(admin.tenant_id)
    .bind(server_key_id)
    .bind(&name)
    .bind(json!(scopes))
    .bind(expires_at)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "server_api_key.update",
            resource_type: "server_api_key",
            resource_id: Some(server_key_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(server_key_audit_json(&before)),
            after_json: Some(json!({
                "id": server_key_id,
                "app_id": before.app_id,
                "name": &name,
                "key_prefix": &before.key_prefix,
                "status": &before.status,
                "scopes": scopes,
                "expires_at": expires_at,
            })),
            metadata_json: json!({
                "app_id": before.app_id,
                "key_prefix": &before.key_prefix,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let server_api_key = find_key(&state, admin.tenant_id, server_key_id).await?;

    Ok(Json(ApiResponse::ok(
        ServerApiKeyResponse { server_api_key },
        request_id.to_string(),
    )))
}

pub async fn revoke_server_api_key(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(server_key_id): Path<Uuid>,
) -> Result<Json<ApiResponse<ServerApiKeyResponse>>, AppError> {
    ensure_admin_permission(&admin, "server_api_key:update")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before = find_key_for_update(&mut transaction, admin.tenant_id, server_key_id).await?;
    if before.status == "revoked" {
        return Err(AppError::already_revoked("server api key already revoked"));
    }

    sqlx::query(
        r#"
        update server_api_keys
        set status = 'revoked',
            revoked_at = now()
        where tenant_id = $1
          and id = $2
        "#,
    )
    .bind(admin.tenant_id)
    .bind(server_key_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "server_api_key.revoke",
            resource_type: "server_api_key",
            resource_id: Some(server_key_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(server_key_audit_json(&before)),
            after_json: Some(json!({
                "id": before.id,
                "app_id": before.app_id,
                "name": &before.name,
                "key_prefix": &before.key_prefix,
                "status": "revoked",
                "scopes": before.scopes,
                "expires_at": before.expires_at,
            })),
            metadata_json: json!({
                "app_id": before.app_id,
                "key_prefix": &before.key_prefix,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let server_api_key = find_key(&state, admin.tenant_id, server_key_id).await?;

    Ok(Json(ApiResponse::ok(
        ServerApiKeyResponse { server_api_key },
        request_id.to_string(),
    )))
}

pub async fn authenticate_server_key(
    state: &AppState,
    headers: &HeaderMap,
    required_scope: &str,
) -> Result<ServerApiKeyContext, AppError> {
    let token = bearer_token(headers)?;
    if !token.starts_with(SERVER_API_KEY_PREFIX) {
        return Err(AppError::invalid_credentials());
    }
    let key_hash = hash_token(&state.config.security.token_hash_pepper, token)?;

    let record = sqlx::query_as::<_, ServerApiKeyAuthRecord>(
        r#"
        select
          k.id,
          k.tenant_id,
          k.app_id,
          k.scopes
        from server_api_keys k
        join tenants t
          on t.id = k.tenant_id
        join applications a
          on a.id = k.app_id
          and a.tenant_id = k.tenant_id
        where k.key_hash = $1
          and k.status = 'active'
          and k.revoked_at is null
          and (k.expires_at is null or k.expires_at > now())
          and t.status = 'active'
          and a.status = 'active'
          and a.deleted_at is null
        "#,
    )
    .bind(key_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::invalid_credentials)?;

    let scopes = scopes_from_value(&record.scopes)?;
    if !scopes.iter().any(|scope| scope == required_scope) {
        return Err(AppError::forbidden(format!(
            "missing server api scope: {required_scope}"
        )));
    }

    sqlx::query(
        r#"
        update server_api_keys
        set last_used_at = now()
        where id = $1
        "#,
    )
    .bind(record.id)
    .execute(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(ServerApiKeyContext {
        server_key_id: record.id,
        tenant_id: record.tenant_id,
        app_id: record.app_id,
        scopes,
    })
}

pub async fn ensure_server_customer_subscription(
    state: &AppState,
    context: &ServerApiKeyContext,
    customer_id: Uuid,
) -> Result<(), AppError> {
    ensure_active_customer(state, context.tenant_id, customer_id).await?;
    let subscription = SubscriptionRepository::new(state.db.clone())
        .find_active_for_customer(context.tenant_id, context.app_id, customer_id, Utc::now())
        .await?;

    if subscription.is_some() {
        return Ok(());
    }

    Err(AppError::subscription_inactive(
        "active subscription required",
    ))
}

pub fn customer_id_from_headers(headers: &HeaderMap) -> Result<Uuid, AppError> {
    let value = headers
        .get(CUSTOMER_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::validation_failed(format!("{CUSTOMER_ID_HEADER} header is required"))
        })?;

    Uuid::parse_str(value)
        .map_err(|_| AppError::validation_failed(format!("{CUSTOMER_ID_HEADER} header is invalid")))
}

pub fn ai_invoke_scope() -> &'static str {
    SCOPE_AI_INVOKE
}

async fn list_keys(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Option<Uuid>,
    include_history: bool,
) -> Result<Vec<ServerApiKey>, AppError> {
    sqlx::query_as::<_, ServerApiKey>(
        r#"
        select
          k.id,
          k.app_id,
          a.name as app_name,
          a.app_key,
          k.name,
          k.key_prefix,
          k.status,
          k.scopes,
          k.expires_at,
          k.last_used_at,
          k.created_at,
          k.revoked_at
        from server_api_keys k
        join applications a
          on a.id = k.app_id
          and a.tenant_id = k.tenant_id
        where k.tenant_id = $1
          and ($2::uuid is null or k.app_id = $2)
          and ($3::bool or k.status = 'active')
        order by k.created_at desc, k.id desc
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(include_history)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn find_key(state: &AppState, tenant_id: Uuid, id: Uuid) -> Result<ServerApiKey, AppError> {
    sqlx::query_as::<_, ServerApiKey>(&server_key_select_sql(
        r#"
        where k.tenant_id = $1
          and k.id = $2
        "#,
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("server api key not found"))
}

async fn find_key_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    id: Uuid,
) -> Result<ServerApiKey, AppError> {
    sqlx::query_as::<_, ServerApiKey>(&server_key_select_sql(
        r#"
        where k.tenant_id = $1
          and k.id = $2
        for update of k
        "#,
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("server api key not found"))
}

fn server_key_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
          k.id,
          k.app_id,
          a.name as app_name,
          a.app_key,
          k.name,
          k.key_prefix,
          k.status,
          k.scopes,
          k.expires_at,
          k.last_used_at,
          k.created_at,
          k.revoked_at
        from server_api_keys k
        join applications a
          on a.id = k.app_id
          and a.tenant_id = k.tenant_id
        {where_clause}
        "#
    )
}

async fn ensure_active_application(
    state: &AppState,
    tenant_id: Uuid,
    app_id: Uuid,
) -> Result<(), AppError> {
    let status = sqlx::query_scalar::<_, String>(
        r#"
        select status
        from applications
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::app_not_found)?;

    if status != "active" {
        return Err(AppError::app_disabled());
    }

    Ok(())
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
        return Err(AppError::account_disabled());
    }

    Ok(())
}

fn normalize_key_name(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.len() > MAX_KEY_NAME_LEN || value.contains('\0') {
        return Err(AppError::validation_failed(
            "server api key name is invalid",
        ));
    }

    Ok(value.to_owned())
}

fn normalize_scopes(scopes: Option<Vec<String>>) -> Result<Vec<String>, AppError> {
    let scopes = scopes.unwrap_or_else(|| vec![SCOPE_AI_INVOKE.to_owned()]);
    if scopes.is_empty() {
        return Err(AppError::validation_failed(
            "server api key scopes are invalid",
        ));
    }

    let mut normalized = Vec::new();
    for scope in scopes {
        let scope = scope.trim();
        if !ALLOWED_SCOPES.contains(&scope) {
            return Err(AppError::validation_failed(format!(
                "server api key scope is not supported: {scope}"
            )));
        }
        if !normalized.iter().any(|value| value == scope) {
            normalized.push(scope.to_owned());
        }
    }
    if normalized.len() > ALLOWED_SCOPES.len() {
        return Err(AppError::validation_failed(
            "server api key scopes are invalid",
        ));
    }

    Ok(normalized)
}

fn scopes_from_value(value: &Value) -> Result<Vec<String>, AppError> {
    let scopes = value
        .as_array()
        .ok_or_else(|| AppError::validation_failed("server api key scopes are invalid"))?
        .iter()
        .map(|scope| {
            scope
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| AppError::validation_failed("server api key scopes are invalid"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    normalize_scopes(Some(scopes))
}

fn generate_server_api_key() -> String {
    format!("{SERVER_API_KEY_PREFIX}{}", generate_token())
}

fn display_prefix(key: &str) -> String {
    key.chars().take(DISPLAY_PREFIX_LEN).collect()
}

fn server_key_audit_json(record: &ServerApiKey) -> Value {
    json!({
        "id": record.id,
        "app_id": record.app_id,
        "name": &record.name,
        "key_prefix": &record.key_prefix,
        "status": &record.status,
        "scopes": record.scopes.clone(),
        "expires_at": record.expires_at,
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
    AppError::dependency(format!("server api key database error: {error}"))
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;
    use uuid::Uuid;

    use super::{
        bearer_token, customer_id_from_headers, display_prefix, generate_server_api_key,
        normalize_key_name, normalize_scopes, scopes_from_value, CUSTOMER_ID_HEADER,
        SCOPE_AI_INVOKE,
    };

    #[test]
    fn generated_key_has_server_prefix() {
        let key = generate_server_api_key();

        assert!(key.starts_with("ehsk_"));
        assert!(key.len() > 40);
        assert_eq!(display_prefix(&key).chars().count(), 18);
    }

    #[test]
    fn key_name_is_validated() {
        assert_eq!(
            normalize_key_name(" Web Backend ").expect("name"),
            "Web Backend"
        );
        assert!(normalize_key_name("").is_err());
    }

    #[test]
    fn scopes_default_to_ai_invoke_and_deduplicate() {
        assert_eq!(
            normalize_scopes(None).expect("default scopes"),
            vec![SCOPE_AI_INVOKE]
        );
        assert_eq!(
            normalize_scopes(Some(vec![
                SCOPE_AI_INVOKE.to_owned(),
                format!(" {SCOPE_AI_INVOKE} "),
            ]))
            .expect("dedup scopes"),
            vec![SCOPE_AI_INVOKE]
        );
        assert!(normalize_scopes(Some(vec![concat!("admin", ":", "all").to_owned()])).is_err());
        assert_eq!(
            scopes_from_value(&json!([SCOPE_AI_INVOKE])).expect("json scopes"),
            vec![SCOPE_AI_INVOKE]
        );
    }

    #[test]
    fn bearer_token_reads_server_authorization_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::AUTHORIZATION,
            HeaderValue::from_static("Bearer ehsk_test"),
        );

        assert_eq!(bearer_token(&headers).expect("bearer"), "ehsk_test");
    }

    #[test]
    fn customer_id_header_is_required_and_parsed() {
        let customer_id = Uuid::new_v4();
        let mut headers = HeaderMap::new();
        headers.insert(
            CUSTOMER_ID_HEADER,
            HeaderValue::from_str(&customer_id.to_string()).expect("header"),
        );

        assert_eq!(
            customer_id_from_headers(&headers).expect("customer id"),
            customer_id
        );
        assert!(customer_id_from_headers(&HeaderMap::new()).is_err());
    }
}
