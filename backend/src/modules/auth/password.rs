use std::env;

use axum::{extract::State, http::HeaderMap, Extension, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::{
        password::{hash_password, verify_password},
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        outbox,
        team::{model::TeamMember, repository::TeamMemberRepository},
    },
    rate_limit,
    state::AppState,
};

const ADMIN_PASSWORD_RESET_PURPOSE: &str = "admin_password_reset";
const ADMIN_PASSWORD_RESET_TTL_HOURS: i64 = 2;

#[derive(Debug, Deserialize)]
pub struct ChangePasswordRequest {
    pub old_password: String,
    pub new_password: String,
}

#[derive(Debug, Deserialize)]
pub struct PasswordResetRequest {
    pub email: String,
}

#[derive(Debug, Deserialize)]
pub struct PasswordResetConfirmRequest {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Serialize)]
pub struct PasswordOkResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct ChangePasswordResponse {
    pub ok: bool,
    pub revoked_sessions: u64,
}

#[derive(Debug, Serialize)]
pub struct PasswordResetConfirmResponse {
    pub ok: bool,
    pub revoked_sessions: u64,
}

#[derive(Debug, Clone)]
pub struct AdminPasswordResetCliInput {
    pub email: String,
    pub tenant_slug: Option<String>,
    pub new_password: Option<String>,
    pub disable_mfa: bool,
}

#[derive(Debug, Clone)]
pub struct AdminPasswordResetCliResult {
    pub tenant_id: Uuid,
    pub team_member_id: Uuid,
    pub tenant_slug: String,
    pub email: String,
    pub generated_password: Option<String>,
    pub revoked_sessions: u64,
    pub revoked_refresh_tokens: u64,
    pub mfa_disabled: bool,
}

#[derive(Debug, FromRow)]
struct PasswordResetSubject {
    token_id: Uuid,
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: String,
}

#[derive(Debug, FromRow)]
struct ResetCandidate {
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: String,
    member_status: String,
    tenant_status: String,
}

#[derive(Debug, FromRow)]
struct AdminPasswordResetCliCandidate {
    tenant_id: Uuid,
    team_member_id: Uuid,
    tenant_slug: String,
    email: String,
}

impl AdminPasswordResetCliInput {
    pub fn from_env() -> Result<Self, AppError> {
        Ok(Self {
            email: required_env("RESET_ADMIN_EMAIL")?,
            tenant_slug: optional_non_empty_env("RESET_ADMIN_TENANT_SLUG"),
            new_password: optional_non_empty_env("RESET_ADMIN_PASSWORD"),
            disable_mfa: parse_env_bool("RESET_ADMIN_DISABLE_MFA"),
        })
    }
}

pub async fn change_password(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<ChangePasswordRequest>,
) -> Result<Json<ApiResponse<ChangePasswordResponse>>, AppError> {
    validate_new_password(&payload.new_password)?;
    let member = TeamMemberRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, admin.team_member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    let old_password_ok = verify_password(&payload.old_password, &member.password_hash)?;
    if !old_password_ok {
        return Err(AppError::invalid_credentials());
    }
    if verify_password(&payload.new_password, &member.password_hash)? {
        return Err(AppError::validation_failed(
            "new_password must be different from old_password",
        ));
    }

    let password_hash = hash_password(&payload.new_password)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    update_member_password_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        &password_hash,
    )
    .await?;
    let revoked_sessions = revoke_admin_sessions_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        Some(admin.session_id),
    )
    .await?;
    let revoked_refresh_tokens = revoke_admin_refresh_tokens_in_transaction(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        Some(admin.session_id),
    )
    .await?;
    audit_password_change(
        &mut transaction,
        &admin,
        &request_id,
        "team_member.password.change",
        revoked_sessions,
        revoked_refresh_tokens,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        ChangePasswordResponse {
            ok: true,
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn request_password_reset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
    Json(payload): Json<PasswordResetRequest>,
) -> Result<Json<ApiResponse<PasswordOkResponse>>, AppError> {
    let email = normalize_email(&payload.email);
    let ip = rate_limit::client_ip(&headers);
    rate_limit::check_fixed_window(
        &state,
        rate_limit::login_key(email.as_deref().unwrap_or("invalid-email"), &ip),
        state.config.security.login_rate_limit_max,
        state.config.security.login_rate_limit_window_seconds,
        AppError::password_reset_rate_limited,
    )
    .await?;

    if let Some(email) = email {
        let candidates = find_reset_candidates(&state, &email).await?;
        if candidates.len() == 1 {
            let candidate = &candidates[0];
            if candidate.member_status == "active" && candidate.tenant_status == "active" {
                create_password_reset_for_candidate(&state, &request_id, candidate).await?;
            }
        }
    }

    Ok(Json(ApiResponse::ok(
        PasswordOkResponse { ok: true },
        request_id.to_string(),
    )))
}

pub async fn confirm_password_reset(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<PasswordResetConfirmRequest>,
) -> Result<Json<ApiResponse<PasswordResetConfirmResponse>>, AppError> {
    let token = normalize_token(&payload.token)?;
    validate_new_password(&payload.new_password)?;
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let password_hash = hash_password(&payload.new_password)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let subject = find_active_reset_subject_for_update(&mut transaction, &token_hash).await?;
    let before =
        find_member_for_update(&mut transaction, subject.tenant_id, subject.team_member_id)
            .await?
            .ok_or_else(AppError::user_not_found)?;
    update_member_password_in_transaction(
        &mut transaction,
        subject.tenant_id,
        subject.team_member_id,
        &password_hash,
    )
    .await?;
    consume_reset_token(&mut transaction, subject.token_id).await?;
    let revoked_sessions = revoke_admin_sessions_in_transaction(
        &mut transaction,
        subject.tenant_id,
        subject.team_member_id,
        None,
    )
    .await?;
    let revoked_refresh_tokens = revoke_admin_refresh_tokens_in_transaction(
        &mut transaction,
        subject.tenant_id,
        subject.team_member_id,
        None,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(subject.tenant_id),
            actor_type: "anonymous",
            actor_id: None,
            action: "team_member.password_reset.confirm",
            resource_type: "team_member",
            resource_id: Some(subject.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_audit_json(&before)),
            after_json: Some(serde_json::json!({
                "id": subject.team_member_id,
                "email": subject.email,
                "password_changed": true,
            })),
            metadata_json: serde_json::json!({
                "one_time_token_id": subject.token_id,
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        PasswordResetConfirmResponse {
            ok: true,
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

pub async fn reset_admin_password_cli(
    pool: &PgPool,
    input: AdminPasswordResetCliInput,
) -> Result<AdminPasswordResetCliResult, AppError> {
    let email = normalize_email(&input.email)
        .ok_or_else(|| AppError::validation_failed("email is invalid"))?;
    let (new_password, generated) = match input.new_password {
        Some(password) => {
            validate_new_password(&password)?;
            (password, false)
        }
        None => (generate_cli_password(), true),
    };
    let password_hash = hash_password(&new_password)?;
    let candidates =
        find_admin_password_reset_cli_candidates(pool, &email, input.tenant_slug.as_deref())
            .await?;

    if candidates.is_empty() {
        return Err(AppError::user_not_found());
    }
    if candidates.len() > 1 {
        return Err(AppError::config(
            "multiple active admins match RESET_ADMIN_EMAIL; set RESET_ADMIN_TENANT_SLUG",
        ));
    }

    let candidate = &candidates[0];
    let mut transaction = pool.begin().await.map_err(map_db_error)?;
    let before = find_member_for_update(
        &mut transaction,
        candidate.tenant_id,
        candidate.team_member_id,
    )
    .await?
    .ok_or_else(AppError::user_not_found)?;

    update_member_password_in_transaction(
        &mut transaction,
        candidate.tenant_id,
        candidate.team_member_id,
        &password_hash,
    )
    .await?;
    let mfa_disabled = if input.disable_mfa {
        disable_member_mfa_in_transaction(
            &mut transaction,
            candidate.tenant_id,
            candidate.team_member_id,
        )
        .await?
    } else {
        false
    };
    let revoked_sessions = revoke_admin_sessions_in_transaction(
        &mut transaction,
        candidate.tenant_id,
        candidate.team_member_id,
        None,
    )
    .await?;
    let revoked_refresh_tokens = revoke_admin_refresh_tokens_in_transaction(
        &mut transaction,
        candidate.tenant_id,
        candidate.team_member_id,
        None,
    )
    .await?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(candidate.tenant_id),
            actor_type: "system",
            actor_id: None,
            action: "team_member.password_reset.cli",
            resource_type: "team_member",
            resource_id: Some(candidate.team_member_id),
            ip: None,
            user_agent: None,
            request_id: None,
            before_json: Some(team_member_audit_json(&before)),
            after_json: Some(serde_json::json!({
                "id": candidate.team_member_id,
                "email": candidate.email,
                "password_changed": true,
                "mfa_disabled": mfa_disabled,
            })),
            metadata_json: serde_json::json!({
                "tenant_slug": candidate.tenant_slug,
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await?;

    transaction.commit().await.map_err(map_db_error)?;

    Ok(AdminPasswordResetCliResult {
        tenant_id: candidate.tenant_id,
        team_member_id: candidate.team_member_id,
        tenant_slug: candidate.tenant_slug.clone(),
        email: candidate.email.clone(),
        generated_password: generated.then_some(new_password),
        revoked_sessions,
        revoked_refresh_tokens,
        mfa_disabled,
    })
}

async fn create_password_reset_for_candidate(
    state: &AppState,
    request_id: &RequestId,
    candidate: &ResetCandidate,
) -> Result<(), AppError> {
    let token = generate_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let expires_at = Utc::now() + Duration::hours(ADMIN_PASSWORD_RESET_TTL_HOURS);
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let token_id = create_reset_token_in_transaction(
        &mut transaction,
        candidate.tenant_id,
        candidate.team_member_id,
        &candidate.email,
        &token_hash,
        expires_at,
    )
    .await?;
    let outbox_event_id = outbox::enqueue_admin_password_reset_email(
        &mut transaction,
        state,
        candidate.tenant_id,
        &candidate.email,
        &token,
        expires_at,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(candidate.tenant_id),
            actor_type: "anonymous",
            actor_id: None,
            action: "team_member.password_reset.request",
            resource_type: "team_member",
            resource_id: Some(candidate.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: None,
            metadata_json: serde_json::json!({
                "one_time_token_id": token_id,
                "outbox_event_id": outbox_event_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(())
}

async fn find_reset_candidates(
    state: &AppState,
    email: &str,
) -> Result<Vec<ResetCandidate>, AppError> {
    sqlx::query_as::<_, ResetCandidate>(
        r#"
        select
          tm.tenant_id,
          tm.id as team_member_id,
          tm.email,
          tm.status as member_status,
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
    .map_err(map_db_error)
}

async fn create_reset_token_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: &str,
    token_hash: &str,
    expires_at: DateTime<Utc>,
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
          expires_at,
          metadata
        )
        values ($1, $2, $3, 'team_member', $4, lower($5), $6, $7, '{}'::jsonb)
        "#,
    )
    .bind(id)
    .bind(tenant_id)
    .bind(ADMIN_PASSWORD_RESET_PURPOSE)
    .bind(team_member_id)
    .bind(email)
    .bind(token_hash)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(id)
}

async fn find_admin_password_reset_cli_candidates(
    pool: &PgPool,
    email: &str,
    tenant_slug: Option<&str>,
) -> Result<Vec<AdminPasswordResetCliCandidate>, AppError> {
    sqlx::query_as::<_, AdminPasswordResetCliCandidate>(
        r#"
        select
          t.id as tenant_id,
          tm.id as team_member_id,
          t.slug as tenant_slug,
          tm.email
        from team_members tm
        join tenants t on t.id = tm.tenant_id
        where lower(tm.email) = lower($1)
          and tm.status = 'active'
          and tm.deleted_at is null
          and t.status = 'active'
          and t.deleted_at is null
          and ($2::text is null or t.slug = $2)
        order by tm.created_at asc
        "#,
    )
    .bind(email)
    .bind(tenant_slug)
    .fetch_all(pool)
    .await
    .map_err(map_db_error)
}

async fn find_active_reset_subject_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    token_hash: &str,
) -> Result<PasswordResetSubject, AppError> {
    sqlx::query_as::<_, PasswordResetSubject>(
        r#"
        select
          ott.id as token_id,
          ott.tenant_id as tenant_id,
          ott.subject_id as team_member_id,
          ott.email as email
        from one_time_tokens ott
        where ott.purpose = $1
          and ott.subject_type = 'team_member'
          and ott.token_hash = $2
          and ott.tenant_id is not null
          and ott.subject_id is not null
          and ott.email is not null
          and ott.expires_at > now()
          and ott.consumed_at is null
          and ott.revoked_at is null
        for update
        "#,
    )
    .bind(ADMIN_PASSWORD_RESET_PURPOSE)
    .bind(token_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::password_reset_token_invalid)
}

async fn find_member_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
) -> Result<Option<TeamMember>, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        select
          id,
          tenant_id,
          email,
          password_hash,
          name,
          phone,
          avatar,
          status,
          email_verified,
          mfa_enabled,
          mfa_secret_encrypted,
          last_login_at,
          last_login_ip::text as last_login_ip,
          created_at,
          updated_at,
          deleted_at
        from team_members
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        for update
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn disable_member_mfa_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
) -> Result<bool, AppError> {
    let disabled = sqlx::query_scalar::<_, bool>(
        r#"
        update team_members
        set
          mfa_enabled = false,
          mfa_secret_encrypted = null,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
          and mfa_enabled = true
        returning true
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(disabled.unwrap_or(false))
}

async fn update_member_password_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    password_hash: &str,
) -> Result<(), AppError> {
    let updated = sqlx::query_scalar::<_, Uuid>(
        r#"
        update team_members
        set
          password_hash = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning id
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(password_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    if updated.is_none() {
        return Err(AppError::user_not_found());
    }

    Ok(())
}

async fn consume_reset_token(
    transaction: &mut Transaction<'_, Postgres>,
    token_id: Uuid,
) -> Result<(), AppError> {
    let consumed = sqlx::query_scalar::<_, Uuid>(
        r#"
        update one_time_tokens
        set consumed_at = now()
        where id = $1
          and expires_at > now()
          and consumed_at is null
          and revoked_at is null
        returning id
        "#,
    )
    .bind(token_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    if consumed.is_none() {
        return Err(AppError::password_reset_token_invalid());
    }

    Ok(())
}

async fn revoke_admin_sessions_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    except_session_id: Option<Uuid>,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_sessions
        set revoked_at = now()
        where tenant_id = $1
          and team_member_id = $2
          and revoked_at is null
          and ($3::uuid is null or id <> $3)
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(except_session_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_admin_refresh_tokens_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    except_session_id: Option<Uuid>,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_refresh_tokens rt
        set revoked_at = now()
        from admin_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and s.team_member_id = $2
          and ($3::uuid is null or s.id <> $3)
          and rt.used_at is null
          and rt.revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(except_session_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn audit_password_change(
    transaction: &mut Transaction<'_, Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    revoked_sessions: u64,
    revoked_refresh_tokens: u64,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "team_member",
            resource_id: Some(admin.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(serde_json::json!({
                "id": admin.team_member_id,
                "email": admin.email,
                "password_changed": true,
            })),
            metadata_json: serde_json::json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await
}

fn team_member_audit_json(member: &TeamMember) -> serde_json::Value {
    serde_json::json!({
        "id": member.id,
        "email": member.email,
        "status": member.status,
        "email_verified": member.email_verified,
        "mfa_enabled": member.mfa_enabled,
    })
}

fn normalize_email(email: &str) -> Option<String> {
    let email = email.trim().to_lowercase();
    (email.contains('@') && email.len() <= 254).then_some(email)
}

fn generate_cli_password() -> String {
    format!("EntitleHub1!{}", generate_token())
}

fn required_env(key: &str) -> Result<String, AppError> {
    let value = env::var(key)
        .map_err(|_| AppError::config(format!("{key} environment variable is required")))?;
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::config(format!(
            "{key} environment variable cannot be empty",
        )));
    }

    Ok(value.to_owned())
}

fn optional_non_empty_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_env_bool(key: &str) -> bool {
    matches!(
        env::var(key).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn normalize_token(token: &str) -> Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::invalid_request("token is required"));
    }

    Ok(token.to_owned())
}

pub fn validate_new_password(password: &str) -> Result<(), AppError> {
    if password.len() < 10 {
        return Err(AppError::validation_failed(
            "password must be at least 10 characters",
        ));
    }
    let has_letter = password
        .chars()
        .any(|character| character.is_ascii_alphabetic());
    let has_digit = password.chars().any(|character| character.is_ascii_digit());
    let has_special = password
        .chars()
        .any(|character| !character.is_ascii_alphanumeric());
    if !(has_letter && has_digit && has_special) {
        return Err(AppError::validation_failed(
            "password must include letters, digits, and special characters",
        ));
    }
    let lower = password.to_ascii_lowercase();
    let weak = [
        "password",
        "password123",
        "password@123",
        "1234567890",
        "qwerty12345",
        "admin12345",
    ];
    if weak.iter().any(|candidate| candidate == &lower) {
        return Err(AppError::weak_password());
    }

    Ok(())
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("auth password database error: {error}"))
}

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    use super::{generate_cli_password, normalize_email, normalize_token, validate_new_password};

    #[test]
    fn password_policy_accepts_strong_password() {
        assert!(validate_new_password("Strong@12345").is_ok());
    }

    #[test]
    fn generated_cli_password_satisfies_password_policy() {
        assert!(validate_new_password(&generate_cli_password()).is_ok());
    }

    #[test]
    fn password_policy_rejects_missing_requirements() {
        assert!(validate_new_password("short@1").is_err());
        assert!(validate_new_password("NoDigits!!!!").is_err());
        assert!(validate_new_password("NoSpecial123").is_err());
        assert!(matches!(
            validate_new_password("password@123"),
            Err(AppError::WeakPassword(_))
        ));
    }

    #[test]
    fn normalize_email_is_optional_to_avoid_enumeration() {
        assert_eq!(
            normalize_email(" Admin@Example.COM "),
            Some("admin@example.com".to_owned())
        );
        assert_eq!(normalize_email("invalid"), None);
    }

    #[test]
    fn normalize_token_trims_and_rejects_blank() {
        assert_eq!(normalize_token(" token ").expect("token"), "token");
        assert!(normalize_token(" ").is_err());
    }
}
