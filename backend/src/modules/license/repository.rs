use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::license::model::{License, LicenseListQuery, NewLicense},
};

#[derive(Clone)]
pub struct LicenseRepository {
    pool: PgPool,
}

impl LicenseRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        query: &LicenseListQuery,
    ) -> Result<Vec<License>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;
        let keyword = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let keyword_pattern = keyword.map(|value| format!("%{}%", value.to_lowercase()));
        let status = query
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let include_history = query.include_history.unwrap_or(false);

        sqlx::query_as::<_, License>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type as license_type,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              revoked_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from licenses
            where tenant_id = $1
              and deleted_at is null
              and ($2::uuid is null or app_id = $2)
              and ($3::uuid is null or customer_id = $3)
              and ($4::text is null or status = $4)
              and (
                $4::text is not null
                or $6::bool
                or (
                  status not in ('revoked', 'expired')
                  and (expires_at is null or expires_at > now())
                )
              )
              and (
                $5::text is null
                or lower(type) like $5
                or id::text like $5
              )
            order by created_at desc, id
            limit $7 offset $8
            "#,
        )
        .bind(tenant_id)
        .bind(query.app_id)
        .bind(query.customer_id)
        .bind(status)
        .bind(keyword_pattern)
        .bind(include_history)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn create(&self, license: NewLicense) -> Result<License, AppError> {
        sqlx::query_as::<_, License>(
            r#"
            insert into licenses (
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type,
              max_devices,
              features,
              starts_at,
              expires_at,
              metadata
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type as license_type,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              revoked_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(license.id)
        .bind(license.tenant_id)
        .bind(license.app_id)
        .bind(license.customer_id)
        .bind(license.license_key_hash)
        .bind(license.license_type)
        .bind(license.max_devices)
        .bind(license.features)
        .bind(license.starts_at)
        .bind(license.expires_at)
        .bind(license.metadata)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(&self, tenant_id: Uuid, id: Uuid) -> Result<Option<License>, AppError> {
        sqlx::query_as::<_, License>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type as license_type,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              revoked_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from licenses
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

    pub async fn find_by_app_and_key_hash(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        license_key_hash: &str,
    ) -> Result<Option<License>, AppError> {
        sqlx::query_as::<_, License>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type as license_type,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              revoked_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from licenses
            where tenant_id = $1
              and app_id = $2
              and license_key_hash = $3
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(license_key_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn revoke(&self, tenant_id: Uuid, id: Uuid) -> Result<Option<License>, AppError> {
        self.set_status(tenant_id, id, "revoked", true).await
    }

    pub async fn suspend(&self, tenant_id: Uuid, id: Uuid) -> Result<Option<License>, AppError> {
        self.set_status(tenant_id, id, "suspended", false).await
    }

    pub async fn renew(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<License>, AppError> {
        sqlx::query_as::<_, License>(
            r#"
            update licenses
            set
              status = 'active',
              expires_at = $3,
              revoked_at = null,
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
              and status <> 'revoked'
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type as license_type,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              revoked_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .bind(expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    async fn set_status(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        status: &'static str,
        revoked: bool,
    ) -> Result<Option<License>, AppError> {
        sqlx::query_as::<_, License>(
            r#"
            update licenses
            set
              status = $3,
              revoked_at = case when $4 then now() else revoked_at end,
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
              and status <> $3
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              license_key_hash,
              type as license_type,
              status,
              max_devices,
              features,
              starts_at,
              expires_at,
              revoked_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .bind(status)
        .bind(revoked)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("license repository database error: {error}"))
}
