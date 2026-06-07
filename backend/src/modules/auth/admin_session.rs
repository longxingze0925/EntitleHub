use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct AdminSession {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub team_member_id: Uuid,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct AdminRefreshToken {
    pub id: Uuid,
    pub session_id: Uuid,
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct AdminSessionSummary {
    pub id: Uuid,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct AdminSessionRepository {
    pool: PgPool,
}

impl AdminSessionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
        user_agent: Option<String>,
        ip: Option<&str>,
        expires_at: DateTime<Utc>,
    ) -> Result<AdminSession, AppError> {
        sqlx::query_as::<_, AdminSession>(
            r#"
            insert into admin_sessions (
              id,
              tenant_id,
              team_member_id,
              user_agent,
              ip,
              expires_at
            )
            values ($1, $2, $3, $4, $5::inet, $6)
            returning
              id,
              tenant_id,
              team_member_id,
              user_agent,
              created_at,
              last_seen_at,
              expires_at,
              revoked_at
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(tenant_id)
        .bind(team_member_id)
        .bind(user_agent)
        .bind(ip)
        .bind(expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(&self, session_id: Uuid) -> Result<Option<AdminSession>, AppError> {
        sqlx::query_as::<_, AdminSession>(
            r#"
            select
              id,
              tenant_id,
              team_member_id,
              user_agent,
              created_at,
              last_seen_at,
              expires_at,
              revoked_at
            from admin_sessions
            where id = $1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_for_member(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
    ) -> Result<Vec<AdminSessionSummary>, AppError> {
        sqlx::query_as::<_, AdminSessionSummary>(
            r#"
            select
              id,
              user_agent,
              ip::text as ip,
              created_at,
              last_seen_at,
              expires_at,
              revoked_at
            from admin_sessions
            where tenant_id = $1
              and team_member_id = $2
            order by
              (revoked_at is null and expires_at > now()) desc,
              coalesce(last_seen_at, created_at) desc,
              id desc
            limit 50
            "#,
        )
        .bind(tenant_id)
        .bind(team_member_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_refresh_token_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<AdminRefreshToken>, AppError> {
        sqlx::query_as::<_, AdminRefreshToken>(
            r#"
            select
              id,
              session_id,
              token_hash,
              created_at,
              expires_at,
              used_at,
              revoked_at
            from admin_refresh_tokens
            where token_hash = $1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn revoke(&self, session_id: Uuid) -> Result<bool, AppError> {
        let revoked = sqlx::query_scalar::<_, Uuid>(
            r#"
            update admin_sessions
            set revoked_at = now()
            where id = $1
              and revoked_at is null
            returning id
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(revoked.is_some())
    }

    pub async fn revoke_refresh_tokens_for_session(
        &self,
        session_id: Uuid,
    ) -> Result<u64, AppError> {
        sqlx::query(
            r#"
            update admin_refresh_tokens
            set revoked_at = now()
            where session_id = $1
              and used_at is null
              and revoked_at is null
            "#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected())
        .map_err(map_db_error)
    }

    pub async fn revoke_for_member(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
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
        .bind(team_member_id)
        .execute(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(result.rows_affected())
    }
}

pub async fn create_admin_session_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    team_member_id: Uuid,
    user_agent: Option<String>,
    ip: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<AdminSession, AppError> {
    sqlx::query_as::<_, AdminSession>(
        r#"
        insert into admin_sessions (
          id,
          tenant_id,
          team_member_id,
          user_agent,
          ip,
          expires_at
        )
        values ($1, $2, $3, $4, $5::inet, $6)
        returning
          id,
          tenant_id,
          team_member_id,
          user_agent,
          created_at,
          last_seen_at,
          expires_at,
          revoked_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(team_member_id)
    .bind(user_agent)
    .bind(ip)
    .bind(expires_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn find_admin_session_for_update_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
) -> Result<Option<AdminSession>, AppError> {
    sqlx::query_as::<_, AdminSession>(
        r#"
        select
          id,
          tenant_id,
          team_member_id,
          user_agent,
          created_at,
          last_seen_at,
          expires_at,
          revoked_at
        from admin_sessions
        where id = $1
        for update
        "#,
    )
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn revoke_admin_session_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
) -> Result<bool, AppError> {
    let revoked = sqlx::query_scalar::<_, Uuid>(
        r#"
        update admin_sessions
        set revoked_at = now()
        where id = $1
          and revoked_at is null
        returning id
        "#,
    )
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    Ok(revoked.is_some())
}

pub async fn revoke_admin_refresh_tokens_for_session_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_refresh_tokens
        set revoked_at = now()
        where session_id = $1
          and used_at is null
          and revoked_at is null
        "#,
    )
    .bind(session_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

pub async fn create_admin_refresh_token_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
    token_hash: String,
    expires_at: DateTime<Utc>,
) -> Result<AdminRefreshToken, AppError> {
    sqlx::query_as::<_, AdminRefreshToken>(
        r#"
        insert into admin_refresh_tokens (
          id,
          session_id,
          token_hash,
          expires_at
        )
        values ($1, $2, $3, $4)
        returning
          id,
          session_id,
          token_hash,
          created_at,
          expires_at,
          used_at,
          revoked_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(session_id)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn mark_admin_refresh_token_used_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    token_id: Uuid,
) -> Result<(), AppError> {
    let used = sqlx::query_scalar::<_, Uuid>(
        r#"
        update admin_refresh_tokens
        set used_at = now()
        where id = $1
          and used_at is null
          and revoked_at is null
        returning id
        "#,
    )
    .bind(token_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    if used.is_none() {
        return Err(AppError::refresh_reuse_detected());
    }

    Ok(())
}

pub async fn extend_admin_session_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session_id: Uuid,
    expires_at: DateTime<Utc>,
) -> Result<AdminSession, AppError> {
    sqlx::query_as::<_, AdminSession>(
        r#"
        update admin_sessions
        set
          expires_at = $2,
          last_seen_at = now()
        where id = $1
          and revoked_at is null
        returning
          id,
          tenant_id,
          team_member_id,
          user_agent,
          created_at,
          last_seen_at,
          expires_at,
          revoked_at
        "#,
    )
    .bind(session_id)
    .bind(expires_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("admin session repository database error: {error}"))
}
