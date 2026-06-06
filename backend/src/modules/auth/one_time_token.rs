use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct OneTimeToken {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub purpose: String,
    pub subject_type: String,
    pub subject_id: Option<Uuid>,
    pub email: Option<String>,
    pub token_hash: String,
    pub created_by: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub consumed_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewOneTimeToken {
    pub id: Uuid,
    pub tenant_id: Option<Uuid>,
    pub purpose: String,
    pub subject_type: String,
    pub subject_id: Option<Uuid>,
    pub email: Option<String>,
    pub token_hash: String,
    pub created_by: Option<Uuid>,
    pub expires_at: DateTime<Utc>,
    pub metadata: Value,
}

impl NewOneTimeToken {
    pub fn new(
        purpose: impl Into<String>,
        subject_type: impl Into<String>,
        token_hash: impl Into<String>,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: None,
            purpose: purpose.into(),
            subject_type: subject_type.into(),
            subject_id: None,
            email: None,
            token_hash: token_hash.into(),
            created_by: None,
            expires_at,
            metadata: serde_json::json!({}),
        }
    }
}

#[derive(Clone)]
pub struct OneTimeTokenRepository {
    pool: sqlx::PgPool,
}

impl OneTimeTokenRepository {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, token: NewOneTimeToken) -> Result<OneTimeToken, AppError> {
        sqlx::query_as::<_, OneTimeToken>(
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
            values ($1, $2, $3, $4, $5, lower($6), $7, $8, $9, $10)
            returning
              id,
              tenant_id,
              purpose,
              subject_type,
              subject_id,
              email,
              token_hash,
              created_by,
              expires_at,
              consumed_at,
              revoked_at,
              metadata,
              created_at
            "#,
        )
        .bind(token.id)
        .bind(token.tenant_id)
        .bind(token.purpose)
        .bind(token.subject_type)
        .bind(token.subject_id)
        .bind(token.email)
        .bind(token.token_hash)
        .bind(token.created_by)
        .bind(token.expires_at)
        .bind(token.metadata)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_active(
        &self,
        purpose: &str,
        token_hash: &str,
    ) -> Result<Option<OneTimeToken>, AppError> {
        sqlx::query_as::<_, OneTimeToken>(
            r#"
            select
              id,
              tenant_id,
              purpose,
              subject_type,
              subject_id,
              email,
              token_hash,
              created_by,
              expires_at,
              consumed_at,
              revoked_at,
              metadata,
              created_at
            from one_time_tokens
            where purpose = $1
              and token_hash = $2
              and expires_at > now()
              and consumed_at is null
              and revoked_at is null
            "#,
        )
        .bind(purpose)
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn consume(&self, id: Uuid) -> Result<bool, AppError> {
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
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(consumed.is_some())
    }

    pub async fn revoke(&self, id: Uuid) -> Result<bool, AppError> {
        let revoked = sqlx::query_scalar::<_, Uuid>(
            r#"
            update one_time_tokens
            set revoked_at = now()
            where id = $1
              and consumed_at is null
              and revoked_at is null
            returning id
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(revoked.is_some())
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("one-time token repository database error: {error}"))
}
