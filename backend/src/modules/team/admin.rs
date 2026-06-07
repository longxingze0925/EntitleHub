use axum::{
    extract::{Path, Query, State},
    Extension, Json,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{postgres::PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::{
        password::hash_password,
        token::{generate_token, hash_token},
    },
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        iam::role::{RoleRepository, RoleSummary},
        outbox,
        team::{
            model::{NewInvitedTeamMember, TeamMember},
            repository::TeamMemberRepository,
        },
    },
    state::AppState,
};

const TEAM_INVITE_PURPOSE: &str = "team_invite";
const TEAM_INVITE_TTL_HOURS: i64 = 72;

#[derive(Debug, Serialize)]
pub struct TeamMemberListResponse {
    pub items: Vec<TeamMemberResponse>,
}

#[derive(Debug, Serialize)]
pub struct TeamMemberResponse {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub phone: Option<String>,
    pub avatar: Option<String>,
    pub status: String,
    pub email_verified: bool,
    pub mfa_enabled: bool,
    pub roles: Vec<RoleSummary>,
}

#[derive(Debug, Deserialize)]
pub struct InviteTeamMemberRequest {
    pub email: String,
    pub role_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct InviteTeamMemberResponse {
    pub member: TeamMemberResponse,
    pub invitation: InvitationResponse,
}

#[derive(Debug, Serialize)]
pub struct InvitationResponse {
    pub token: String,
    pub expires_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemberRolesRequest {
    pub role_codes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateMemberRolesResponse {
    pub member: TeamMemberResponse,
}

#[derive(Debug, Serialize)]
pub struct DisableMemberResponse {
    pub member: TeamMemberResponse,
    pub revoked_sessions: u64,
}

#[derive(Debug, Deserialize)]
pub struct TeamMemberListQuery {
    pub include_history: Option<bool>,
}

pub async fn list_members(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Query(query): Query<TeamMemberListQuery>,
) -> Result<Json<ApiResponse<TeamMemberListResponse>>, AppError> {
    ensure_admin_permission(&admin, "member:read")?;

    let members = TeamMemberRepository::new(state.db.clone())
        .list_by_tenant(admin.tenant_id, query.include_history.unwrap_or(false))
        .await?;
    let mut items = Vec::with_capacity(members.len());

    for member in members {
        items.push(member_response(&state.db, admin.tenant_id, member).await?);
    }

    Ok(Json(ApiResponse::ok(
        TeamMemberListResponse { items },
        request_id.to_string(),
    )))
}

pub async fn invite_member(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<InviteTeamMemberRequest>,
) -> Result<Json<ApiResponse<InviteTeamMemberResponse>>, AppError> {
    ensure_admin_permission(&admin, "member:invite")?;

    let email = normalize_email(&payload.email)?;
    let role_codes = normalize_role_codes(payload.role_codes)?;
    let roles = resolve_roles(&state.db, admin.tenant_id, &role_codes).await?;

    let member_repository = TeamMemberRepository::new(state.db.clone());
    if member_repository
        .find_by_email(admin.tenant_id, &email)
        .await?
        .is_some()
    {
        return Err(AppError::duplicate_email());
    }

    let placeholder_password = generate_token();
    let placeholder_hash = hash_password(&placeholder_password)?;
    let token = generate_token();
    let token_hash = hash_token(&state.config.security.token_hash_pepper, &token)?;
    let expires_at = Utc::now() + Duration::hours(TEAM_INVITE_TTL_HOURS);
    let new_member = NewInvitedTeamMember::new(admin.tenant_id, email.clone(), placeholder_hash);

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let member = create_invited_member(&mut transaction, new_member).await?;
    replace_member_roles(&mut transaction, admin.tenant_id, member.id, &roles).await?;
    let invitation_id = create_invitation_token(
        &mut transaction,
        admin.tenant_id,
        member.id,
        &email,
        &token_hash,
        admin.team_member_id,
        expires_at,
    )
    .await?;
    let outbox_event_id = outbox::enqueue_team_invite_email(
        &mut transaction,
        &state,
        admin.tenant_id,
        &email,
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
            action: "team_member.invite",
            resource_type: "team_member",
            resource_id: Some(member.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: None,
            after_json: Some(json!({
                "id": member.id,
                "email": member.email,
                "status": member.status,
                "role_codes": role_codes,
            })),
            metadata_json: json!({
                "invitation_id": invitation_id,
                "outbox_event_id": outbox_event_id,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let response_member = member_response(&state.db, admin.tenant_id, member).await?;

    Ok(Json(ApiResponse::ok(
        InviteTeamMemberResponse {
            member: response_member,
            invitation: InvitationResponse { token, expires_at },
        },
        request_id.to_string(),
    )))
}

pub async fn update_member_roles(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(member_id): Path<Uuid>,
    Json(payload): Json<UpdateMemberRolesRequest>,
) -> Result<Json<ApiResponse<UpdateMemberRolesResponse>>, AppError> {
    ensure_admin_permission(&admin, "member:update")?;

    let role_codes = normalize_role_codes(payload.role_codes)?;
    let roles = resolve_roles(&state.db, admin.tenant_id, &role_codes).await?;
    let member_repository = TeamMemberRepository::new(state.db.clone());
    let member = member_repository
        .find_by_id(admin.tenant_id, member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;

    let current_role_codes = RoleRepository::new(state.db.clone())
        .list_codes_for_member(admin.tenant_id, member_id)
        .await?;

    ensure_owner_role_change_allowed(&admin, member_id, &current_role_codes, &role_codes)?;
    ensure_not_removing_last_owner(&state, admin.tenant_id, &current_role_codes, &role_codes)
        .await?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    replace_member_roles(&mut transaction, admin.tenant_id, member_id, &roles).await?;
    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.roles.update",
            resource_type: "team_member",
            resource_id: Some(member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "role_codes": current_role_codes,
            })),
            after_json: Some(json!({
                "role_codes": role_codes,
            })),
            metadata_json: json!({}),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let response_member = member_response(&state.db, admin.tenant_id, member).await?;

    Ok(Json(ApiResponse::ok(
        UpdateMemberRolesResponse {
            member: response_member,
        },
        request_id.to_string(),
    )))
}

pub async fn disable_member(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(member_id): Path<Uuid>,
) -> Result<Json<ApiResponse<DisableMemberResponse>>, AppError> {
    ensure_admin_permission(&admin, "member:disable")?;

    let member_repository = TeamMemberRepository::new(state.db.clone());
    let member = member_repository
        .find_by_id(admin.tenant_id, member_id)
        .await?
        .ok_or_else(AppError::user_not_found)?;
    let current_role_codes = RoleRepository::new(state.db.clone())
        .list_codes_for_member(admin.tenant_id, member_id)
        .await?;

    ensure_owner_target_allowed(&admin, &current_role_codes)?;
    ensure_not_disabling_last_owner(&state, admin.tenant_id, &member, &current_role_codes).await?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let Some(disabled_member) =
        disable_member_in_transaction(&mut transaction, admin.tenant_id, member_id).await?
    else {
        return Err(AppError::conflict("team member already disabled"));
    };
    let revoked_sessions =
        revoke_member_sessions_in_transaction(&mut transaction, admin.tenant_id, member_id).await?;
    let revoked_refresh_tokens =
        revoke_member_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id, member_id)
            .await?;

    audit::record(
        &mut transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action: "team_member.disable",
            resource_type: "team_member",
            resource_id: Some(member_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: Some(json!({
                "status": member.status,
            })),
            after_json: Some(json!({
                "status": disabled_member.status,
            })),
            metadata_json: json!({
                "revoked_sessions": revoked_sessions,
                "revoked_refresh_tokens": revoked_refresh_tokens,
            }),
        },
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    let response_member = member_response(&state.db, admin.tenant_id, disabled_member).await?;

    Ok(Json(ApiResponse::ok(
        DisableMemberResponse {
            member: response_member,
            revoked_sessions,
        },
        request_id.to_string(),
    )))
}

async fn member_response(
    pool: &PgPool,
    tenant_id: Uuid,
    member: TeamMember,
) -> Result<TeamMemberResponse, AppError> {
    let roles = RoleRepository::new(pool.clone())
        .list_for_member(tenant_id, member.id)
        .await?;

    Ok(TeamMemberResponse {
        id: member.id,
        email: member.email,
        name: member.name,
        phone: member.phone,
        avatar: member.avatar,
        status: member.status,
        email_verified: member.email_verified,
        mfa_enabled: member.mfa_enabled,
        roles,
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

fn normalize_email(email: &str) -> Result<String, AppError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return Err(AppError::validation_failed("email is invalid"));
    }

    Ok(email)
}

fn normalize_role_codes(role_codes: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut normalized = role_codes
        .into_iter()
        .map(|code| code.trim().to_lowercase())
        .filter(|code| !code.is_empty())
        .collect::<Vec<_>>();

    normalized.sort();
    normalized.dedup();

    if normalized.is_empty() {
        return Err(AppError::validation_failed("at least one role is required"));
    }

    Ok(normalized)
}

async fn resolve_roles(
    pool: &PgPool,
    tenant_id: Uuid,
    role_codes: &[String],
) -> Result<Vec<crate::modules::iam::role::RoleRecord>, AppError> {
    let roles = RoleRepository::new(pool.clone())
        .find_by_codes(tenant_id, role_codes)
        .await?;

    if roles.len() != role_codes.len() {
        return Err(AppError::validation_failed("role code is invalid"));
    }

    Ok(roles)
}

fn ensure_owner_target_allowed(
    admin: &AdminContext,
    target_role_codes: &[String],
) -> Result<(), AppError> {
    if !target_role_codes.iter().any(|code| code == "owner") {
        return Ok(());
    }

    if admin_has_owner_role(admin) {
        return Ok(());
    }

    Err(AppError::forbidden("owner cannot be changed by non-owner"))
}

fn ensure_owner_role_change_allowed(
    admin: &AdminContext,
    _target_member_id: Uuid,
    current_role_codes: &[String],
    next_role_codes: &[String],
) -> Result<(), AppError> {
    let touches_owner = current_role_codes.iter().any(|code| code == "owner")
        || next_role_codes.iter().any(|code| code == "owner");

    if !touches_owner {
        return Ok(());
    }

    if admin_has_owner_role(admin) {
        return Ok(());
    }

    Err(AppError::forbidden("owner cannot be changed by non-owner"))
}

async fn ensure_not_removing_last_owner(
    state: &AppState,
    tenant_id: Uuid,
    current_role_codes: &[String],
    next_role_codes: &[String],
) -> Result<(), AppError> {
    let removes_owner = current_role_codes.iter().any(|code| code == "owner")
        && !next_role_codes.iter().any(|code| code == "owner");

    if !removes_owner {
        return Ok(());
    }

    ensure_owner_count_above_one(state, tenant_id).await
}

async fn ensure_not_disabling_last_owner(
    state: &AppState,
    tenant_id: Uuid,
    member: &TeamMember,
    role_codes: &[String],
) -> Result<(), AppError> {
    if member.status != "active" || !role_codes.iter().any(|code| code == "owner") {
        return Ok(());
    }

    ensure_owner_count_above_one(state, tenant_id).await
}

async fn ensure_owner_count_above_one(state: &AppState, tenant_id: Uuid) -> Result<(), AppError> {
    let owner_count = RoleRepository::new(state.db.clone())
        .active_owner_count(tenant_id)
        .await?;

    if owner_count <= 1 {
        return Err(AppError::business_rule_failed(
            "cannot remove or disable the last owner",
        ));
    }

    Ok(())
}

fn admin_has_owner_role(admin: &AdminContext) -> bool {
    admin.roles.iter().any(|role| role == "owner")
}

async fn disable_member_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    member_id: Uuid,
) -> Result<Option<TeamMember>, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        update team_members
        set
          status = 'disabled',
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
          and status <> 'disabled'
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
    .bind(member_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn revoke_member_sessions_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    member_id: Uuid,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        update admin_sessions
        set revoked_at = now()
        where tenant_id = $1
          and team_member_id = $2
          and revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(member_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(result.rows_affected())
}

async fn revoke_member_refresh_tokens_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    member_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_refresh_tokens rt
        set revoked_at = now()
        from admin_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and s.team_member_id = $2
          and rt.used_at is null
          and rt.revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(member_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn create_invited_member(
    transaction: &mut Transaction<'_, Postgres>,
    member: NewInvitedTeamMember,
) -> Result<TeamMember, AppError> {
    sqlx::query_as::<_, TeamMember>(
        r#"
        insert into team_members (
          id,
          tenant_id,
          email,
          password_hash,
          name,
          status
        )
        values ($1, $2, lower($3), $4, $5, 'invited')
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
    .bind(member.id)
    .bind(member.tenant_id)
    .bind(member.email)
    .bind(member.password_hash)
    .bind(member.name)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn replace_member_roles(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    member_id: Uuid,
    roles: &[crate::modules::iam::role::RoleRecord],
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        delete from team_member_roles
        where team_member_id in (
          select id
          from team_members
          where tenant_id = $1
            and id = $2
            and deleted_at is null
        )
        "#,
    )
    .bind(tenant_id)
    .bind(member_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    for role in roles {
        sqlx::query(
            r#"
            insert into team_member_roles (
              team_member_id,
              role_id
            )
            select tm.id, r.id
            from team_members tm
            join roles r
              on r.tenant_id = tm.tenant_id
             and r.id = $3
             and r.deleted_at is null
            where tm.tenant_id = $1
              and tm.id = $2
              and tm.deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(member_id)
        .bind(role.id)
        .execute(&mut **transaction)
        .await
        .map_err(map_db_error)?;
    }

    Ok(())
}

async fn create_invitation_token(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    member_id: Uuid,
    email: &str,
    token_hash: &str,
    created_by: Uuid,
    expires_at: chrono::DateTime<Utc>,
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
    .bind(TEAM_INVITE_PURPOSE)
    .bind(member_id)
    .bind(email)
    .bind(token_hash)
    .bind(created_by)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(id)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("team admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::modules::auth::session::AdminContext;

    use super::{ensure_admin_permission, normalize_email, normalize_role_codes};

    #[test]
    fn normalize_role_codes_trims_sorts_and_deduplicates() {
        let role_codes = normalize_role_codes(vec![
            " developer ".to_owned(),
            "admin".to_owned(),
            "developer".to_owned(),
        ])
        .expect("roles should normalize");

        assert_eq!(role_codes, vec!["admin", "developer"]);
    }

    #[test]
    fn normalize_email_rejects_invalid_email() {
        assert!(normalize_email("not-an-email").is_err());
    }

    #[test]
    fn permission_check_uses_loaded_admin_permissions() {
        let admin = AdminContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            team_member_id: Uuid::nil(),
            email: "owner@example.com".to_owned(),
            name: "Owner".to_owned(),
            email_verified: true,
            mfa_enabled: false,
            tenant_name: "Default".to_owned(),
            roles: vec!["owner".to_owned()],
            permissions: vec!["member:read".to_owned()],
        };

        assert!(ensure_admin_permission(&admin, "member:read").is_ok());
        assert!(ensure_admin_permission(&admin, "member:update").is_err());
    }
}
