use axum::{
    extract::State,
    http::{
        header::{SET_COOKIE, USER_AGENT},
        HeaderMap,
    },
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        auth::{
            admin_session::{
                create_admin_refresh_token_in_transaction, extend_admin_session_in_transaction,
                mark_admin_refresh_token_used_in_transaction, AdminSessionRepository,
            },
            csrf::{build_csrf_cookie, issue_csrf_token},
            session::{
                build_refresh_cookie, build_session_cookie, read_cookie, ADMIN_REFRESH_COOKIE,
            },
        },
        iam::{permission::PermissionRepository, role::RoleRepository},
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub user: RefreshUser,
    pub tenant: RefreshTenant,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RefreshUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub email_verified: bool,
    pub mfa_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct RefreshTenant {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, FromRow)]
struct RefreshPrincipal {
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: String,
    name: String,
    email_verified: bool,
    mfa_enabled: bool,
    member_status: String,
    tenant_name: String,
    tenant_status: String,
}

pub async fn refresh(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let refresh_token = read_cookie(&headers, ADMIN_REFRESH_COOKIE)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(AppError::unauthenticated)?;
    let ip = rate_limit::client_ip(&headers);
    let token_hash = hash_token(&state.config.security.refresh_token_pepper, refresh_token)?;
    let repository = AdminSessionRepository::new(state.db.clone());

    let Some(stored_token) = repository.find_refresh_token_by_hash(&token_hash).await? else {
        rate_limit::check_fixed_window(
            &state,
            rate_limit::refresh_key("invalid-admin-token", &ip),
            state.config.security.refresh_rate_limit_max,
            state.config.security.refresh_rate_limit_window_seconds,
            AppError::refresh_rate_limited,
        )
        .await?;
        return Err(AppError::unauthenticated());
    };

    rate_limit::check_fixed_window(
        &state,
        rate_limit::refresh_key(&stored_token.session_id.to_string(), &ip),
        state.config.security.refresh_rate_limit_max,
        state.config.security.refresh_rate_limit_window_seconds,
        AppError::refresh_rate_limited,
    )
    .await?;

    if stored_token.used_at.is_some() || stored_token.revoked_at.is_some() {
        repository.revoke(stored_token.session_id).await?;
        repository
            .revoke_refresh_tokens_for_session(stored_token.session_id)
            .await?;
        return Err(AppError::refresh_reuse_detected());
    }

    let now = Utc::now();
    if stored_token.expires_at <= now {
        return Err(AppError::session_expired());
    }

    let session = repository
        .find_by_id(stored_token.session_id)
        .await?
        .ok_or_else(AppError::session_expired)?;
    if session.revoked_at.is_some() {
        return Err(AppError::session_expired());
    }

    let principal = load_refresh_principal(&state, session.id).await?;
    if principal.member_status != "active" {
        return Err(AppError::account_disabled());
    }
    if principal.tenant_status != "active" {
        return Err(AppError::tenant_forbidden());
    }

    let next_refresh_token = generate_token();
    let next_refresh_token_hash = hash_token(
        &state.config.security.refresh_token_pepper,
        &next_refresh_token,
    )?;
    let ttl_seconds = state.config.security.admin_session_ttl_seconds;
    let next_expires_at = now + Duration::seconds(ttl_seconds);
    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    if let Err(error) =
        mark_admin_refresh_token_used_in_transaction(&mut transaction, stored_token.id).await
    {
        let reuse_detected = matches!(&error, AppError::RefreshReuseDetected(_));
        transaction.rollback().await.map_err(map_db_error)?;
        if reuse_detected {
            repository.revoke(stored_token.session_id).await?;
            repository
                .revoke_refresh_tokens_for_session(stored_token.session_id)
                .await?;
        }
        return Err(error);
    }
    create_admin_refresh_token_in_transaction(
        &mut transaction,
        session.id,
        next_refresh_token_hash,
        next_expires_at,
    )
    .await?;
    let session =
        extend_admin_session_in_transaction(&mut transaction, session.id, next_expires_at).await?;
    update_session_user_agent_in_transaction(&mut transaction, session.id, user_agent).await?;
    transaction.commit().await.map_err(map_db_error)?;

    let permissions = PermissionRepository::new(state.db.clone())
        .list_for_member(principal.tenant_id, principal.team_member_id)
        .await?;
    let roles = RoleRepository::new(state.db.clone())
        .list_codes_for_member(principal.tenant_id, principal.team_member_id)
        .await?;
    let body = ApiResponse::ok(
        RefreshResponse {
            user: RefreshUser {
                id: principal.team_member_id,
                email: principal.email,
                name: principal.name,
                email_verified: principal.email_verified,
                mfa_enabled: principal.mfa_enabled,
            },
            tenant: RefreshTenant {
                id: principal.tenant_id,
                name: principal.tenant_name,
            },
            roles,
            permissions,
        },
        request_id.to_string(),
    );

    let session_cookie = build_session_cookie(
        session.id,
        &state.config.security.session_secret,
        state.config.security.cookie_secure,
        ttl_seconds,
    )?;
    let refresh_cookie = build_refresh_cookie(
        &next_refresh_token,
        state.config.security.cookie_secure,
        ttl_seconds,
    )?;
    let csrf_token = issue_csrf_token(&state.config.security.csrf_secret)?;
    let csrf_cookie = build_csrf_cookie(
        &csrf_token,
        state.config.security.cookie_secure,
        ttl_seconds,
    )?;
    let mut response = Json(body).into_response();
    response.headers_mut().insert(SET_COOKIE, session_cookie);
    response.headers_mut().append(SET_COOKIE, refresh_cookie);
    response.headers_mut().append(SET_COOKIE, csrf_cookie);

    Ok(response)
}

async fn load_refresh_principal(
    state: &AppState,
    session_id: Uuid,
) -> Result<RefreshPrincipal, AppError> {
    sqlx::query_as::<_, RefreshPrincipal>(
        r#"
        select
          s.tenant_id,
          s.team_member_id,
          tm.email,
          tm.name,
          tm.email_verified,
          tm.mfa_enabled,
          tm.status as member_status,
          t.name as tenant_name,
          t.status as tenant_status
        from admin_sessions s
        join team_members tm
          on tm.id = s.team_member_id
         and tm.tenant_id = s.tenant_id
         and tm.deleted_at is null
        join tenants t
          on t.id = s.tenant_id
         and t.deleted_at is null
        where s.id = $1
        "#,
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::session_expired)
}

async fn update_session_user_agent_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session_id: Uuid,
    user_agent: Option<String>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update admin_sessions
        set user_agent = coalesce($2, user_agent)
        where id = $1
        "#,
    )
    .bind(session_id)
    .bind(user_agent)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("admin refresh database error: {error}"))
}

#[cfg(test)]
mod tests {
    use axum::http::{header::COOKIE, HeaderMap, HeaderValue};

    use crate::modules::auth::session::{read_cookie, ADMIN_REFRESH_COOKIE};

    #[test]
    fn refresh_cookie_can_be_read_from_cookie_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            COOKIE,
            HeaderValue::from_static("admin_session=session; admin_refresh=refresh-token"),
        );

        assert_eq!(
            read_cookie(&headers, ADMIN_REFRESH_COOKIE),
            Some("refresh-token")
        );
    }
}
