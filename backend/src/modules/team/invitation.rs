use axum::{extract::State, Extension, Json};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::{password::hash_password, token::hash_token},
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::password::validate_new_password,
        team::model::TeamMember,
    },
    state::AppState,
};

const TEAM_INVITE_PURPOSE: &str = "team_invite";

#[derive(Debug, Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub name: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AcceptInvitationResponse {
    pub member: AcceptedTeamMemberResponse,
}

#[derive(Debug, Serialize)]
pub struct AcceptedTeamMemberResponse {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub name: String,
    pub status: String,
    pub email_verified: bool,
}

#[derive(Debug, FromRow)]
struct InvitationSubject {
    token_id: Uuid,
    tenant_id: Uuid,
    team_member_id: Uuid,
    email: String,
}

pub async fn accept_invitation(
    State(state): State<AppState>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<AcceptInvitationRequest>,
) -> Result<Json<ApiResponse<AcceptInvitationResponse>>, AppError> {
    let token = normalize_token(&payload.token)?;
    let name = normalize_name(&payload.name)?;
    validate_new_password(&payload.password)?;
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let password_hash = hash_password(&payload.password)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let subject = find_active_invitation_for_update(&mut transaction, &token_hash).await?;
    let before =
        find_invited_member_for_update(&mut transaction, subject.tenant_id, subject.team_member_id)
            .await?
            .ok_or_else(AppError::invite_token_invalid)?;
    if !before.email.eq_ignore_ascii_case(&subject.email) {
        return Err(AppError::invite_token_invalid());
    }

    let member = activate_invited_member(
        &mut transaction,
        subject.tenant_id,
        subject.team_member_id,
        &name,
        &password_hash,
    )
    .await?;
    consume_invitation_token(&mut transaction, subject.token_id).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(subject.tenant_id),
            actor_type: "anonymous",
            actor_id: None,
            action: "team_member.invite.accept",
            resource_type: "team_member",
            resource_id: Some(subject.team_member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(team_member_audit_json(&before)),
            after_json: Some(team_member_audit_json(&member)),
            metadata_json: serde_json::json!({
                "one_time_token_id": subject.token_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        AcceptInvitationResponse {
            member: AcceptedTeamMemberResponse {
                id: member.id,
                tenant_id: member.tenant_id,
                email: member.email,
                name: member.name,
                status: member.status,
                email_verified: member.email_verified,
            },
        },
        request_id.to_string(),
    )))
}

async fn find_active_invitation_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    token_hash: &str,
) -> Result<InvitationSubject, AppError> {
    sqlx::query_as::<_, InvitationSubject>(
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
    .bind(TEAM_INVITE_PURPOSE)
    .bind(token_hash)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(AppError::invite_token_invalid)
}

async fn find_invited_member_for_update(
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
          and status = 'invited'
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

async fn activate_invited_member(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    name: &str,
    password_hash: &str,
) -> Result<TeamMember, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        update team_members
        set
          name = $3,
          password_hash = $4,
          status = 'active',
          email_verified = true,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = 'invited'
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
    .bind(name)
    .bind(password_hash)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn consume_invitation_token(
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
        return Err(AppError::invite_token_invalid());
    }

    Ok(())
}

fn team_member_audit_json(member: &TeamMember) -> serde_json::Value {
    serde_json::json!({
        "id": member.id,
        "email": member.email,
        "name": member.name,
        "status": member.status,
        "email_verified": member.email_verified,
    })
}

fn normalize_token(token: &str) -> Result<String, AppError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::invalid_request("token is required"));
    }

    Ok(token.to_owned())
}

fn normalize_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(AppError::validation_failed("name is invalid"));
    }

    Ok(name.to_owned())
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("team invitation database error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{normalize_name, normalize_token};

    #[test]
    fn normalize_token_trims_and_rejects_blank() {
        assert_eq!(normalize_token(" token ").expect("token"), "token");
        assert!(normalize_token(" ").is_err());
    }

    #[test]
    fn normalize_name_trims_and_rejects_blank() {
        assert_eq!(normalize_name(" Developer ").expect("name"), "Developer");
        assert!(normalize_name(" ").is_err());
    }
}
