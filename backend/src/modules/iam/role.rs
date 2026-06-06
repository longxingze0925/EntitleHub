use serde::Serialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct RoleSummary {
    pub id: Uuid,
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct RoleRecord {
    pub id: Uuid,
    pub code: String,
    pub name: String,
}

#[derive(Clone)]
pub struct RoleRepository {
    pool: PgPool,
}

impl RoleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_for_member(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
    ) -> Result<Vec<RoleSummary>, AppError> {
        sqlx::query_as::<_, RoleSummary>(
            r#"
            select distinct r.id, r.code, r.name
            from team_member_roles tmr
            join roles r
              on r.id = tmr.role_id
             and r.deleted_at is null
            join team_members tm
              on tm.id = tmr.team_member_id
             and tm.tenant_id = r.tenant_id
             and tm.deleted_at is null
            where tm.tenant_id = $1
              and tm.id = $2
            order by r.code
            "#,
        )
        .bind(tenant_id)
        .bind(team_member_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_codes_for_member(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
    ) -> Result<Vec<String>, AppError> {
        Ok(self
            .list_for_member(tenant_id, team_member_id)
            .await?
            .into_iter()
            .map(|role| role.code)
            .collect())
    }

    pub async fn find_by_codes(
        &self,
        tenant_id: Uuid,
        role_codes: &[String],
    ) -> Result<Vec<RoleRecord>, AppError> {
        sqlx::query_as::<_, RoleRecord>(
            r#"
            select id, code, name
            from roles
            where tenant_id = $1
              and code = any($2)
              and deleted_at is null
            order by code
            "#,
        )
        .bind(tenant_id)
        .bind(role_codes)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_by_tenant(&self, tenant_id: Uuid) -> Result<Vec<RoleSummary>, AppError> {
        sqlx::query_as::<_, RoleSummary>(
            r#"
            select id, code, name
            from roles
            where tenant_id = $1
              and deleted_at is null
            order by builtin desc, code asc
            "#,
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn active_owner_count(&self, tenant_id: Uuid) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>(
            r#"
            select count(distinct tm.id)
            from team_members tm
            join team_member_roles tmr
              on tmr.team_member_id = tm.id
            join roles r
              on r.id = tmr.role_id
             and r.tenant_id = tm.tenant_id
             and r.code = 'owner'
             and r.deleted_at is null
            where tm.tenant_id = $1
              and tm.status = 'active'
              and tm.deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("role repository database error: {error}"))
}
