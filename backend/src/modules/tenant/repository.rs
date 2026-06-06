use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::tenant::model::{NewTenant, Tenant},
};

#[derive(Clone)]
pub struct TenantRepository {
    pool: PgPool,
}

impl TenantRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, tenant: NewTenant) -> Result<Tenant, AppError> {
        sqlx::query_as::<_, Tenant>(
            r#"
            insert into tenants (
              id,
              name,
              slug,
              plan,
              max_applications,
              max_team_members,
              max_customers,
              metadata
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8)
            returning
              id,
              name,
              slug,
              status,
              plan,
              max_applications,
              max_team_members,
              max_customers,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant.id)
        .bind(tenant.name)
        .bind(tenant.slug)
        .bind(tenant.plan)
        .bind(tenant.max_applications)
        .bind(tenant.max_team_members)
        .bind(tenant.max_customers)
        .bind(tenant.metadata)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Tenant>, AppError> {
        sqlx::query_as::<_, Tenant>(
            r#"
            select
              id,
              name,
              slug,
              status,
              plan,
              max_applications,
              max_team_members,
              max_customers,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from tenants
            where id = $1
              and deleted_at is null
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_slug(&self, slug: &str) -> Result<Option<Tenant>, AppError> {
        sqlx::query_as::<_, Tenant>(
            r#"
            select
              id,
              name,
              slug,
              status,
              plan,
              max_applications,
              max_team_members,
              max_customers,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from tenants
            where slug = $1
              and deleted_at is null
            "#,
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn update_name(&self, id: Uuid, name: &str) -> Result<Option<Tenant>, AppError> {
        sqlx::query_as::<_, Tenant>(
            r#"
            update tenants
            set
              name = $2,
              updated_at = now()
            where id = $1
              and deleted_at is null
            returning
              id,
              name,
              slug,
              status,
              plan,
              max_applications,
              max_team_members,
              max_customers,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn soft_delete(&self, id: Uuid) -> Result<bool, AppError> {
        let deleted = sqlx::query_scalar::<_, Uuid>(
            r#"
            update tenants
            set
              status = 'deleted',
              deleted_at = now(),
              updated_at = now()
            where id = $1
              and deleted_at is null
            returning id
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)?;

        Ok(deleted.is_some())
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("tenant repository database error: {error}"))
}
