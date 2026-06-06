use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::client_auth::model::{
        ClientRefreshToken, ClientSession, NewClientRefreshToken, NewClientSession,
    },
};

#[derive(Clone)]
pub struct ClientAuthRepository {
    pool: PgPool,
}

impl ClientAuthRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_refresh_token_by_hash(
        &self,
        token_hash: &str,
    ) -> Result<Option<ClientRefreshToken>, AppError> {
        sqlx::query_as::<_, ClientRefreshToken>(
            r#"
            select
              id,
              session_id,
              token_hash,
              created_at,
              expires_at,
              used_at,
              revoked_at
            from client_refresh_tokens
            where token_hash = $1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_session_by_id(
        &self,
        session_id: Uuid,
    ) -> Result<Option<ClientSession>, AppError> {
        sqlx::query_as::<_, ClientSession>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              device_id,
              machine_id,
              auth_mode,
              user_agent,
              client_ip::text as client_ip,
              last_used_at,
              expires_at,
              revoked_at,
              created_at
            from client_sessions
            where id = $1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn revoke_session(&self, session_id: Uuid) -> Result<u64, AppError> {
        sqlx::query(
            r#"
            update client_sessions
            set revoked_at = now()
            where id = $1
              and revoked_at is null
            "#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map(|result| result.rows_affected())
        .map_err(map_db_error)
    }

    pub async fn touch_session(&self, session_id: Uuid) -> Result<(), AppError> {
        sqlx::query(
            r#"
            update client_sessions
            set last_used_at = now()
            where id = $1
            "#,
        )
        .bind(session_id)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(map_db_error)
    }
}

pub async fn create_client_session_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    session: NewClientSession,
) -> Result<ClientSession, AppError> {
    sqlx::query_as::<_, ClientSession>(
        r#"
        insert into client_sessions (
          id,
          tenant_id,
          app_id,
          customer_id,
          device_id,
          machine_id,
          auth_mode,
          user_agent,
          client_ip,
          expires_at
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9::inet, $10)
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          device_id,
          machine_id,
          auth_mode,
          user_agent,
          client_ip::text as client_ip,
          last_used_at,
          expires_at,
          revoked_at,
          created_at
        "#,
    )
    .bind(session.id)
    .bind(session.tenant_id)
    .bind(session.app_id)
    .bind(session.customer_id)
    .bind(session.device_id)
    .bind(session.machine_id)
    .bind(session.auth_mode)
    .bind(session.user_agent)
    .bind(session.client_ip)
    .bind(session.expires_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn create_client_refresh_token_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    token: NewClientRefreshToken,
) -> Result<ClientRefreshToken, AppError> {
    sqlx::query_as::<_, ClientRefreshToken>(
        r#"
        insert into client_refresh_tokens (
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
    .bind(token.id)
    .bind(token.session_id)
    .bind(token.token_hash)
    .bind(token.expires_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn mark_refresh_token_used_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    token_id: Uuid,
) -> Result<(), AppError> {
    let used = sqlx::query_scalar::<_, Uuid>(
        r#"
        update client_refresh_tokens
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

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("client auth repository database error: {error}"))
}
