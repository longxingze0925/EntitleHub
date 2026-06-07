use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::application::model::{
        Application, ApplicationListQuery, NewApplication, NewSigningKey, SigningKey,
        UpdateApplication,
    },
};

#[derive(Clone)]
pub struct ApplicationRepository {
    pool: PgPool,
}

impl ApplicationRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        query: &ApplicationListQuery,
    ) -> Result<Vec<Application>, AppError> {
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

        sqlx::query_as::<_, Application>(
            r#"
            select
              id,
              tenant_id,
              name,
              slug,
              app_key,
              app_secret_hash,
              auth_mode,
              status,
              heartbeat_interval_seconds,
              offline_tolerance_seconds,
              max_devices_default,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from applications
            where tenant_id = $1
              and deleted_at is null
              and ($2::text is null or status = $2)
              and ($2::text is not null or $4::bool or status not in ('disabled', 'archived'))
              and (
                $3::text is null
                or lower(name) like $3
                or lower(coalesce(slug, '')) like $3
                or lower(app_key) like $3
              )
            order by created_at desc, id
            limit $5 offset $6
            "#,
        )
        .bind(tenant_id)
        .bind(status)
        .bind(keyword_pattern)
        .bind(include_history)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Application>, AppError> {
        sqlx::query_as::<_, Application>(
            r#"
            select
              id,
              tenant_id,
              name,
              slug,
              app_key,
              app_secret_hash,
              auth_mode,
              status,
              heartbeat_interval_seconds,
              offline_tolerance_seconds,
              max_devices_default,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from applications
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

    pub async fn find_by_app_key(&self, app_key: &str) -> Result<Option<Application>, AppError> {
        sqlx::query_as::<_, Application>(
            r#"
            select
              id,
              tenant_id,
              name,
              slug,
              app_key,
              app_secret_hash,
              auth_mode,
              status,
              heartbeat_interval_seconds,
              offline_tolerance_seconds,
              max_devices_default,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from applications
            where app_key = $1
              and deleted_at is null
            "#,
        )
        .bind(app_key)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn update(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        input: UpdateApplication,
    ) -> Result<Option<Application>, AppError> {
        sqlx::query_as::<_, Application>(
            r#"
            update applications
            set
              name = coalesce($3, name),
              slug = coalesce($4, slug),
              auth_mode = coalesce($5, auth_mode),
              status = coalesce($6, status),
              heartbeat_interval_seconds = coalesce($7, heartbeat_interval_seconds),
              offline_tolerance_seconds = coalesce($8, offline_tolerance_seconds),
              max_devices_default = coalesce($9, max_devices_default),
              metadata = coalesce($10, metadata),
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            returning
              id,
              tenant_id,
              name,
              slug,
              app_key,
              app_secret_hash,
              auth_mode,
              status,
              heartbeat_interval_seconds,
              offline_tolerance_seconds,
              max_devices_default,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .bind(input.name)
        .bind(input.slug)
        .bind(input.auth_mode)
        .bind(input.status)
        .bind(input.heartbeat_interval_seconds)
        .bind(input.offline_tolerance_seconds)
        .bind(input.max_devices_default)
        .bind(input.metadata)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_signing_keys(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
    ) -> Result<Vec<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              key_scope,
              kid,
              alg,
              public_key_pem,
              null::jsonb as private_key_envelope,
              status,
              not_before,
              not_after,
              rotated_from_id,
              created_by,
              created_at,
              activated_at,
              retired_at,
              revoked_at
            from signing_keys
            where tenant_id = $1
              and app_id = $2
              and status <> 'revoked'
            order by created_at desc, id
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_public_jwks_keys_for_app_key(
        &self,
        app_key: &str,
    ) -> Result<Vec<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              sk.id,
              sk.tenant_id,
              sk.app_id,
              sk.key_scope,
              sk.kid,
              sk.alg,
              sk.public_key_pem,
              null::jsonb as private_key_envelope,
              sk.status,
              sk.not_before,
              sk.not_after,
              sk.rotated_from_id,
              sk.created_by,
              sk.created_at,
              sk.activated_at,
              sk.retired_at,
              sk.revoked_at
            from signing_keys sk
            join applications a
              on a.id = sk.app_id
             and a.tenant_id = sk.tenant_id
             and a.deleted_at is null
            where a.app_key = $1
              and a.status = 'active'
              and sk.status in ('active', 'retiring')
              and sk.key_scope in ('release_file', 'secure_script', 'app_request')
              and sk.not_before <= now()
              and (sk.not_after is null or sk.not_after > now())
            order by sk.created_at desc, sk.id
            "#,
        )
        .bind(app_key)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_active_signing_key_with_private_envelope(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        key_scope: &str,
    ) -> Result<Option<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              key_scope,
              kid,
              alg,
              public_key_pem,
              private_key_envelope,
              status,
              not_before,
              not_after,
              rotated_from_id,
              created_by,
              created_at,
              activated_at,
              retired_at,
              revoked_at
            from signing_keys
            where tenant_id = $1
              and app_id = $2
              and key_scope = $3
              and status = 'active'
              and private_key_envelope is not null
              and not_before <= now()
              and (not_after is null or not_after > now())
            order by created_at desc, id
            limit 1
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(key_scope)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_active_global_signing_key_with_private_envelope(
        &self,
        key_scope: &str,
    ) -> Result<Option<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              key_scope,
              kid,
              alg,
              public_key_pem,
              private_key_envelope,
              status,
              not_before,
              not_after,
              rotated_from_id,
              created_by,
              created_at,
              activated_at,
              retired_at,
              revoked_at
            from signing_keys
            where tenant_id is null
              and app_id is null
              and key_scope = $1
              and status = 'active'
              and private_key_envelope is not null
              and not_before <= now()
              and (not_after is null or not_after > now())
            order by created_at desc, id
            limit 1
            "#,
        )
        .bind(key_scope)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_public_global_signing_key_by_kid(
        &self,
        key_scope: &str,
        kid: &str,
    ) -> Result<Option<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              key_scope,
              kid,
              alg,
              public_key_pem,
              null::jsonb as private_key_envelope,
              status,
              not_before,
              not_after,
              rotated_from_id,
              created_by,
              created_at,
              activated_at,
              retired_at,
              revoked_at
            from signing_keys
            where tenant_id is null
              and app_id is null
              and key_scope = $1
              and kid = $2
              and status in ('active', 'retiring')
              and not_before <= now()
              and (not_after is null or not_after > now())
            limit 1
            "#,
        )
        .bind(key_scope)
        .bind(kid)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_global_public_jwks_keys(&self) -> Result<Vec<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              key_scope,
              kid,
              alg,
              public_key_pem,
              null::jsonb as private_key_envelope,
              status,
              not_before,
              not_after,
              rotated_from_id,
              created_by,
              created_at,
              activated_at,
              retired_at,
              revoked_at
            from signing_keys
            where tenant_id is null
              and app_id is null
              and key_scope = 'jwt_access_token'
              and status in ('active', 'retiring')
              and not_before <= now()
              and (not_after is null or not_after > now())
            order by created_at desc, id
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list_global_signing_keys(
        &self,
        key_scope: &str,
    ) -> Result<Vec<SigningKey>, AppError> {
        sqlx::query_as::<_, SigningKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              key_scope,
              kid,
              alg,
              public_key_pem,
              null::jsonb as private_key_envelope,
              status,
              not_before,
              not_after,
              rotated_from_id,
              created_by,
              created_at,
              activated_at,
              retired_at,
              revoked_at
            from signing_keys
            where tenant_id is null
              and app_id is null
              and key_scope = $1
              and status <> 'revoked'
            order by created_at desc, id
            "#,
        )
        .bind(key_scope)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

pub async fn create_application_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    application: NewApplication,
) -> Result<Application, AppError> {
    sqlx::query_as::<_, Application>(
        r#"
        insert into applications (
          id,
          tenant_id,
          name,
          slug,
          app_key,
          app_secret_hash,
          auth_mode,
          heartbeat_interval_seconds,
          offline_tolerance_seconds,
          max_devices_default,
          metadata
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        returning
          id,
          tenant_id,
          name,
          slug,
          app_key,
          app_secret_hash,
          auth_mode,
          status,
          heartbeat_interval_seconds,
          offline_tolerance_seconds,
          max_devices_default,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(application.id)
    .bind(application.tenant_id)
    .bind(application.name)
    .bind(application.slug)
    .bind(application.app_key)
    .bind(application.app_secret_hash)
    .bind(application.auth_mode)
    .bind(application.heartbeat_interval_seconds)
    .bind(application.offline_tolerance_seconds)
    .bind(application.max_devices_default)
    .bind(application.metadata)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn update_application_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    id: Uuid,
    input: UpdateApplication,
) -> Result<Option<Application>, AppError> {
    sqlx::query_as::<_, Application>(
        r#"
        update applications
        set
          name = coalesce($3, name),
          slug = coalesce($4, slug),
          auth_mode = coalesce($5, auth_mode),
          status = coalesce($6, status),
          heartbeat_interval_seconds = coalesce($7, heartbeat_interval_seconds),
          offline_tolerance_seconds = coalesce($8, offline_tolerance_seconds),
          max_devices_default = coalesce($9, max_devices_default),
          metadata = coalesce($10, metadata),
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          name,
          slug,
          app_key,
          app_secret_hash,
          auth_mode,
          status,
          heartbeat_interval_seconds,
          offline_tolerance_seconds,
          max_devices_default,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(id)
    .bind(input.name)
    .bind(input.slug)
    .bind(input.auth_mode)
    .bind(input.status)
    .bind(input.heartbeat_interval_seconds)
    .bind(input.offline_tolerance_seconds)
    .bind(input.max_devices_default)
    .bind(input.metadata)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn update_application_secret_hash_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    app_secret_hash: &str,
) -> Result<Application, AppError> {
    sqlx::query_as::<_, Application>(
        r#"
        update applications
        set
          app_secret_hash = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          name,
          slug,
          app_key,
          app_secret_hash,
          auth_mode,
          status,
          heartbeat_interval_seconds,
          offline_tolerance_seconds,
          max_devices_default,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(app_secret_hash)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn retire_active_app_request_keys_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
) -> Result<Vec<SigningKey>, AppError> {
    sqlx::query_as::<_, SigningKey>(
        r#"
        update signing_keys
        set status = 'retiring'
        where tenant_id = $1
          and app_id = $2
          and key_scope = 'app_request'
          and status = 'active'
        returning
          id,
          tenant_id,
          app_id,
          key_scope,
          kid,
          alg,
          public_key_pem,
          null::jsonb as private_key_envelope,
          status,
          not_before,
          not_after,
          rotated_from_id,
          created_by,
          created_at,
          activated_at,
          retired_at,
          revoked_at
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn retire_active_global_signing_keys_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    key_scope: &str,
    not_after: DateTime<Utc>,
) -> Result<Vec<SigningKey>, AppError> {
    sqlx::query_as::<_, SigningKey>(
        r#"
        update signing_keys
        set
          status = 'retiring',
          not_after = case
            when not_after is null or not_after > $2 then $2
            else not_after
          end
        where tenant_id is null
          and app_id is null
          and key_scope = $1
          and status = 'active'
        returning
          id,
          tenant_id,
          app_id,
          key_scope,
          kid,
          alg,
          public_key_pem,
          null::jsonb as private_key_envelope,
          status,
          not_before,
          not_after,
          rotated_from_id,
          created_by,
          created_at,
          activated_at,
          retired_at,
          revoked_at
        "#,
    )
    .bind(key_scope)
    .bind(not_after)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn create_signing_key_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    signing_key: NewSigningKey,
) -> Result<SigningKey, AppError> {
    sqlx::query_as::<_, SigningKey>(
        r#"
        insert into signing_keys (
          id,
          tenant_id,
          app_id,
          key_scope,
          kid,
          public_key_pem,
          private_key_envelope,
          status,
          rotated_from_id,
          created_by,
          activated_at
        )
        values ($1, $2, $3, $4, $5, $6, $7, 'active', $8, $9, now())
        returning
          id,
          tenant_id,
          app_id,
          key_scope,
          kid,
          alg,
          public_key_pem,
          private_key_envelope,
          status,
          not_before,
          not_after,
          rotated_from_id,
          created_by,
          created_at,
          activated_at,
          retired_at,
          revoked_at
        "#,
    )
    .bind(signing_key.id)
    .bind(signing_key.tenant_id)
    .bind(signing_key.app_id)
    .bind(signing_key.key_scope)
    .bind(signing_key.kid)
    .bind(signing_key.public_key_pem)
    .bind(signing_key.private_key_envelope)
    .bind(signing_key.rotated_from_id)
    .bind(signing_key.created_by)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("application repository database error: {error}"))
}
