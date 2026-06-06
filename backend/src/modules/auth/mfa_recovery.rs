use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow)]
pub struct MfaRecoveryCode {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub team_member_id: Uuid,
    pub code_hash: String,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewMfaRecoveryCode {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub team_member_id: Uuid,
    pub code_hash: String,
}

impl NewMfaRecoveryCode {
    pub fn new(tenant_id: Uuid, team_member_id: Uuid, code_hash: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            team_member_id,
            code_hash: code_hash.into(),
        }
    }
}

#[derive(Clone)]
pub struct MfaRecoveryCodeRepository {
    pool: PgPool,
}

impl MfaRecoveryCodeRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn replace_for_member(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
        code_hashes: Vec<String>,
    ) -> Result<(), AppError> {
        let mut transaction = self.pool.begin().await.map_err(map_db_error)?;

        sqlx::query(
            r#"
            update admin_mfa_recovery_codes
            set revoked_at = now()
            where tenant_id = $1
              and team_member_id = $2
              and used_at is null
              and revoked_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(team_member_id)
        .execute(&mut *transaction)
        .await
        .map_err(map_db_error)?;

        for code_hash in code_hashes {
            let code = NewMfaRecoveryCode::new(tenant_id, team_member_id, code_hash);
            sqlx::query(
                r#"
                insert into admin_mfa_recovery_codes (
                  id,
                  tenant_id,
                  team_member_id,
                  code_hash
                )
                values ($1, $2, $3, $4)
                "#,
            )
            .bind(code.id)
            .bind(code.tenant_id)
            .bind(code.team_member_id)
            .bind(code.code_hash)
            .execute(&mut *transaction)
            .await
            .map_err(map_db_error)?;
        }

        transaction.commit().await.map_err(map_db_error)
    }

    pub async fn find_active_hashes(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
    ) -> Result<Vec<MfaRecoveryCode>, AppError> {
        sqlx::query_as::<_, MfaRecoveryCode>(
            r#"
            select
              id,
              tenant_id,
              team_member_id,
              code_hash,
              used_at,
              revoked_at,
              created_at
            from admin_mfa_recovery_codes
            where tenant_id = $1
              and team_member_id = $2
              and used_at is null
              and revoked_at is null
            order by created_at asc
            "#,
        )
        .bind(tenant_id)
        .bind(team_member_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn mark_used(&self, id: Uuid) -> Result<bool, AppError> {
        let used = sqlx::query_scalar::<_, Uuid>(
            r#"
            update admin_mfa_recovery_codes
            set used_at = now()
            where id = $1
              and used_at is null
              and revoked_at is null
            returning id
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(used.is_some())
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("mfa recovery repository database error: {error}"))
}
