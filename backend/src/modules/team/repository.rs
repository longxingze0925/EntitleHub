use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::team::model::{NewTeamMember, TeamMember},
};

#[derive(Clone)]
pub struct TeamMemberRepository {
    pool: PgPool,
}

impl TeamMemberRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, member: NewTeamMember) -> Result<TeamMember, AppError> {
        sqlx::query_as::<_, TeamMember>(
            r#"
            insert into team_members (
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              avatar
            )
            values ($1, $2, lower($3), $4, $5, $6, $7)
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
        .bind(member.phone)
        .bind(member.avatar)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_by_tenant(
        &self,
        tenant_id: Uuid,
        include_history: bool,
    ) -> Result<Vec<TeamMember>, AppError> {
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
              and deleted_at is null
              and ($2::bool or status <> 'disabled')
            order by created_at desc, id
            "#,
        )
        .bind(tenant_id)
        .bind(include_history)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        id: Uuid,
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
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_email(
        &self,
        tenant_id: Uuid,
        email: &str,
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
              and lower(email) = lower($2)
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn disable(&self, tenant_id: Uuid, id: Uuid) -> Result<bool, AppError> {
        let disabled = sqlx::query_scalar::<_, Uuid>(
            r#"
            update team_members
            set
              status = 'disabled',
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
              and status <> 'disabled'
            returning id
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(disabled.is_some())
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("team member repository database error: {error}"))
}
