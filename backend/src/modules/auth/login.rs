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
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    crypto::{
        password::verify_password,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::{
            admin_session::{
                create_admin_refresh_token_in_transaction, create_admin_session_in_transaction,
            },
            csrf::{build_csrf_cookie, issue_csrf_token},
            mfa,
            session::{build_refresh_cookie, build_session_cookie},
        },
        iam::{permission::PermissionRepository, role::RoleRepository},
    },
    rate_limit,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub mfa_code: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: LoginUser,
    pub tenant: LoginTenant,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginUser {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub email_verified: bool,
    pub mfa_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct LoginTenant {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, FromRow)]
struct LoginCandidate {
    team_member_id: Uuid,
    tenant_id: Uuid,
    email: String,
    password_hash: String,
    name: String,
    email_verified: bool,
    member_status: String,
    mfa_enabled: bool,
    mfa_secret_encrypted: Option<String>,
    tenant_name: String,
    tenant_status: String,
}

pub async fn login(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Response, AppError> {
    let ip = rate_limit::client_ip(&headers);
    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    rate_limit::check_fixed_window(
        &state,
        rate_limit::login_key(&payload.email, &ip),
        state.config.security.login_rate_limit_max,
        state.config.security.login_rate_limit_window_seconds,
        AppError::login_rate_limited,
    )
    .await?;

    let candidates = find_login_candidates(&state, &payload.email).await?;
    if candidates.len() != 1 {
        record_login_failure(
            &state,
            &request_id,
            None,
            None,
            &ip,
            user_agent.clone(),
            &payload.email,
            "candidate_not_found",
        )
        .await;
        return Err(AppError::invalid_credentials());
    }

    let candidate = candidates
        .into_iter()
        .next()
        .ok_or_else(AppError::invalid_credentials)?;

    if candidate.member_status != "active" || candidate.tenant_status != "active" {
        record_login_failure(
            &state,
            &request_id,
            Some(candidate.tenant_id),
            Some(candidate.team_member_id),
            &ip,
            user_agent.clone(),
            &candidate.email,
            "inactive_principal",
        )
        .await;
        return Err(AppError::invalid_credentials());
    }

    let password_ok = verify_password(&payload.password, &candidate.password_hash)?;
    if !password_ok {
        record_login_failure(
            &state,
            &request_id,
            Some(candidate.tenant_id),
            Some(candidate.team_member_id),
            &ip,
            user_agent.clone(),
            &candidate.email,
            "invalid_password",
        )
        .await;
        return Err(AppError::invalid_credentials());
    }

    if candidate.mfa_enabled {
        let mfa_code = payload
            .mfa_code
            .as_deref()
            .map(str::trim)
            .filter(|code| !code.is_empty())
            .ok_or_else(AppError::mfa_required)?;
        rate_limit::check_fixed_window(
            &state,
            rate_limit::mfa_key(&candidate.team_member_id.to_string(), &ip),
            state.config.security.login_rate_limit_max,
            state.config.security.login_rate_limit_window_seconds,
            AppError::login_rate_limited,
        )
        .await?;
        let mfa_ok = mfa::verify_member_mfa_code(
            &state,
            candidate.tenant_id,
            candidate.team_member_id,
            candidate.mfa_secret_encrypted.as_deref(),
            mfa_code,
        )
        .await?;
        if !mfa_ok {
            record_login_failure(
                &state,
                &request_id,
                Some(candidate.tenant_id),
                Some(candidate.team_member_id),
                &ip,
                user_agent.clone(),
                &candidate.email,
                "mfa_failed",
            )
            .await;
            return Err(AppError::mfa_failed());
        }
    }

    let expires_at =
        Utc::now() + Duration::seconds(state.config.security.admin_session_ttl_seconds);
    let refresh_token = generate_token();
    let refresh_token_hash =
        hash_token(&state.config.security.refresh_token_pepper, &refresh_token)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let session = create_admin_session_in_transaction(
        &mut transaction,
        candidate.tenant_id,
        candidate.team_member_id,
        user_agent.clone(),
        Some(ip.as_str()),
        expires_at,
    )
    .await?;
    create_admin_refresh_token_in_transaction(
        &mut transaction,
        session.id,
        refresh_token_hash,
        expires_at,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(candidate.tenant_id),
            actor_type: "team_member",
            actor_id: Some(candidate.team_member_id),
            action: "auth.login.success",
            resource_type: "team_member",
            resource_id: Some(candidate.team_member_id),
            ip: Some(ip),
            user_agent,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: None,
            metadata_json: serde_json::json!({
                "session_id": session.id,
                "mfa_enabled": candidate.mfa_enabled,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let permissions = PermissionRepository::new(state.db.clone())
        .list_for_member(candidate.tenant_id, candidate.team_member_id)
        .await?;
    let roles = RoleRepository::new(state.db.clone())
        .list_codes_for_member(candidate.tenant_id, candidate.team_member_id)
        .await?;

    let body = ApiResponse::ok(
        LoginResponse {
            user: LoginUser {
                id: candidate.team_member_id,
                email: candidate.email,
                name: candidate.name,
                email_verified: candidate.email_verified,
                mfa_enabled: candidate.mfa_enabled,
            },
            tenant: LoginTenant {
                id: candidate.tenant_id,
                name: candidate.tenant_name,
            },
            roles,
            permissions,
        },
        request_id.to_string(),
    );

    let cookie = build_session_cookie(
        session.id,
        &state.config.security.session_secret,
        state.config.security.cookie_secure,
        state.config.security.admin_session_ttl_seconds,
    )?;
    let refresh_cookie = build_refresh_cookie(
        &refresh_token,
        state.config.security.cookie_secure,
        state.config.security.admin_session_ttl_seconds,
    )?;
    let csrf_token = issue_csrf_token(&state.config.security.csrf_secret)?;
    let csrf_cookie = build_csrf_cookie(
        &csrf_token,
        state.config.security.cookie_secure,
        state.config.security.admin_session_ttl_seconds,
    )?;
    let mut response = Json(body).into_response();
    response.headers_mut().insert(SET_COOKIE, cookie);
    response.headers_mut().append(SET_COOKIE, refresh_cookie);
    response.headers_mut().append(SET_COOKIE, csrf_cookie);

    Ok(response)
}

async fn find_login_candidates(
    state: &AppState,
    email: &str,
) -> Result<Vec<LoginCandidate>, AppError> {
    sqlx::query_as::<_, LoginCandidate>(
        r#"
        select
          tm.id as team_member_id,
          tm.tenant_id,
          tm.email,
          tm.password_hash,
          tm.name,
          tm.email_verified,
          tm.status as member_status,
          tm.mfa_enabled,
          tm.mfa_secret_encrypted,
          t.name as tenant_name,
          t.status as tenant_status
        from team_members tm
        join tenants t
          on t.id = tm.tenant_id
         and t.deleted_at is null
        where lower(tm.email) = lower($1)
          and tm.deleted_at is null
        limit 2
        "#,
    )
    .bind(email)
    .fetch_all(&state.db)
    .await
    .map_err(|error| AppError::dependency(format!("login query failed: {error}")))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("login database error: {error}"))
}

async fn record_login_failure(
    state: &AppState,
    request_id: &RequestId,
    tenant_id: Option<Uuid>,
    actor_id: Option<Uuid>,
    ip: &str,
    user_agent: Option<String>,
    email: &str,
    reason: &'static str,
) {
    let result = async {
        let mut transaction = state.db.begin().await.map_err(map_db_error)?;
        audit::record(
            &mut transaction,
            AuditLogInput {
                tenant_id,
                actor_type: "team_member",
                actor_id,
                action: "auth.login.failed",
                resource_type: "auth",
                resource_id: actor_id,
                ip: Some(ip.to_owned()),
                user_agent,
                request_id: Some(request_id.to_string()),
                before_json: None,
                after_json: None,
                metadata_json: serde_json::json!({
                    "email": email.trim().to_lowercase(),
                    "reason": reason,
                }),
            },
        )
        .await?;
        transaction.commit().await.map_err(map_db_error)
    }
    .await;

    if let Err(error) = result {
        tracing::warn!(error = %error, "failed to record login failure audit");
    }
}
