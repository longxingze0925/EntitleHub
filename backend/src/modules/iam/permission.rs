use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Clone)]
pub struct PermissionRepository {
    pool: PgPool,
}

impl PermissionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list_for_member(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
    ) -> Result<Vec<String>, AppError> {
        sqlx::query_scalar::<_, String>(
            r#"
            select distinct p.code
            from team_members tm
            join team_member_roles tmr
              on tmr.team_member_id = tm.id
            join roles r
              on r.id = tmr.role_id
             and r.tenant_id = tm.tenant_id
             and r.deleted_at is null
            join role_permissions rp
              on rp.role_id = r.id
            join permissions p
              on p.id = rp.permission_id
            where tm.tenant_id = $1
              and tm.id = $2
              and tm.status = 'active'
              and tm.deleted_at is null
            order by p.code
            "#,
        )
        .bind(tenant_id)
        .bind(team_member_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn member_has_permission(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
        permission_code: &str,
    ) -> Result<bool, AppError> {
        let exists = sqlx::query_scalar::<_, bool>(
            r#"
            select exists (
              select 1
              from team_members tm
              join team_member_roles tmr
                on tmr.team_member_id = tm.id
              join roles r
                on r.id = tmr.role_id
               and r.tenant_id = tm.tenant_id
               and r.deleted_at is null
              join role_permissions rp
                on rp.role_id = r.id
              join permissions p
                on p.id = rp.permission_id
              where tm.tenant_id = $1
                and tm.id = $2
                and p.code = $3
                and tm.status = 'active'
                and tm.deleted_at is null
            )
            "#,
        )
        .bind(tenant_id)
        .bind(team_member_id)
        .bind(permission_code)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(exists)
    }

    pub async fn ensure_permission(
        &self,
        tenant_id: Uuid,
        team_member_id: Uuid,
        permission_code: &str,
    ) -> Result<(), AppError> {
        if self
            .member_has_permission(tenant_id, team_member_id, permission_code)
            .await?
        {
            return Ok(());
        }

        Err(AppError::forbidden(format!(
            "missing permission: {permission_code}"
        )))
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("permission repository database error: {error}"))
}
