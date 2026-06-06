use axum::{extract::State, http::HeaderMap, Extension, Json};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::token::{generate_token, hash_token},
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

const ADMIN_EMAIL_VERIFY_PURPOSE: &str = "email_verify";
const ADMIN_EMAIL_VERIFY_TTL_HOURS: i64 = 24;

#[derive(Debug, Serialize)]
pub struct EmailVerifyRequestResponse {
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct EmailVerifyConfirmRequest {
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct EmailVerifyConfirmResponse {
    pub team_member_id: Uuid,
    pub email_verified: bool,
}

#[derive(Debug, FromRow)]
struct EmailVerifySubject {
    token_id: Uuid,
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: String,
}

pub async fn request_email_verify(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<EmailVerifyRequestResponse>>, AppError> {
    let ip = rate_limit::client_ip(&headers);
    rate_limit::check_fixed_window(
        &state,
        rate_limit::email_verify_key(&admin.team_member_id.to_string(), &ip),
        state.config.security.login_rate_limit_max,
        state.config.security.login_rate_limit_window_seconds,
        AppError::login_rate_limited,
    )
    .await?;

    let member = TeamMemberRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id, admin.team_member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    if member.email_verified {
        return Err(AppError::conflict("team member email already verified"));
    }

    let token = generate_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let expires_at = Utc::now() + Duration::hours(ADMIN_EMAIL_VERIFY_TTL_HOURS);

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let token_id = create_email_verify_token(
        &mut transaction,
        admin.tenant_id,
        admin.team_member_id,
        &member.email,
        &token_hash,
        admin.team_member_id,
        expires_at,
    )
    .await?;
    let outbox_event_id = outbox::enqueue_team_member_email_verify_email(
        &mut transaction,
        &state,
        admin.tenant_id,
        &member.email,
        &token,
        expires_at,
    )
    .await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.email_verify.request",
            resource_type: "team_member",
            resource_id: Some(admin.team_member_id),
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

    Ok(Json(ApiResponse::ok(
        EmailVerifyRequestResponse { expires_at },
        request_id.to_string(),
    )))
}

pub async fn confirm_email_verify(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<EmailVerifyConfirmRequest>,
) -> Result<Json<ApiResponse<EmailVerifyConfirmResponse>>, AppError> {
    let token = normalize_token(&payload.token)?;
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let subject =
        find_active_email_verify_subject_for_update(&mut transaction, &token_hash).await?;
    let before =
        find_member_for_update(&mut transaction, subject.tenant_id, subject.team_member_id)
            .await?
            .ok_or_else(AppError::user_not_found)?;
    let member =
        mark_member_email_verified(&mut transaction, subject.tenant_id, subject.team_member_id)
            .await?;
    consume_token(&mut transaction, subject.token_id).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(subject.tenant_id),
            actor_type: "team_member",
            actor_id: Some(subject.team_member_id),
            action: "team_member.email_verify.confirm",
            resource_type: "team_member",
            resource_id: Some(subject.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_audit_json(&before)),
            after_json: Some(team_member_audit_json(&member)),
            metadata_json: serde_json::json!({
                "one_time_token_id": subject.token_id,
                "email": subject.email,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        EmailVerifyConfirmResponse {
            team_member_id: member.id,
            email_verified: member.email_verified,
        },
        request_id.to_string(),
    )))
}

async fn create_email_verify_token(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: &str,
    token_hash: &str,
    created_by: Uuid,
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
          created_by,
          expires_at,
          metadata
        )
        values ($1, $2, $3, 'team_member', $4, lower($5), $6, $7, $8, '{}'::jsonb)
        "#,
    )
    .bind(id)
    .bind(tenant_id)
    .bind(ADMIN_EMAIL_VERIFY_PURPOSE)
    .bind(team_member_id)
    .bind(email)
    .bind(token_hash)
    .bind(created_by)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(id)
}

async fn find_active_email_verify_subject_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    token_hash: &str,
) -> Result<EmailVerifySubject, AppError> {
    sqlx::query_as::<_, EmailVerifySubject>(
        r#"
        select
          id as token_id,
          tenant_id as tenant_id,
          subject_id as team_member_id,
          email as email
        from one_time_tokens
        where purpose = $1
          and subject_type = 'team_member'
          and token_hash = $2
          and tenant_id is not null
          and subject_id is not null
          and email is not null
          and expires_at > now()
          and consumed_at is null
          and revoked_at is null
        for update
        "#,
    )
    .bind(ADMIN_EMAIL_VERIFY_PURPOSE)
    .bind(token_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::token_invalid("email verify token invalid"))
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

async fn mark_member_email_verified(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
) -> Result<TeamMember, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        update team_members
        set
          email_verified = true,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
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
        "#,
    )
    .bind(tenant_id)
    .bind(team_member_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn consume_token(
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
        return Err(AppError::token_invalid("email verify token invalid"));
    }

    Ok(())
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

fn normalize_token(token: &str) -> Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::invalid_request("token is required"));
    }

    Ok(token.to_owned())
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("auth email verify database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::normalize_token;

    #[test]
    fn normalize_token_trims_value() {
        assert_eq!(normalize_token(" token ").expect("token"), "token");
    }

    #[test]
    fn normalize_token_rejects_blank() {
        assert!(normalize_token(" ").is_err());
    }
}
