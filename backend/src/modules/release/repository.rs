use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::release::model::{
        DownloadToken, NewDownloadToken, NewRelease, NewReleaseFile, Release, ReleaseFile,
        ReleaseListQuery, ReleaseWithFile, UpdateRelease, ValidatedDownload,
    },
};

#[derive(Clone)]
pub struct ReleaseRepository {
    pool: PgPool,
}

impl ReleaseRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_file(&self, file: NewReleaseFile) -> Result<ReleaseFile, AppError> {
        sqlx::query_as::<_, ReleaseFile>(
            r#"
            insert into release_files (
              id,
              tenant_id,
              app_id,
              storage_key,
              file_name,
              file_size,
              sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              metadata
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            returning
              id,
              tenant_id,
              app_id,
              storage_key,
              file_name,
              file_size,
              sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              metadata,
              created_at
            "#,
        )
        .bind(file.id)
        .bind(file.tenant_id)
        .bind(file.app_id)
        .bind(file.storage_key)
        .bind(file.file_name)
        .bind(file.file_size)
        .bind(file.sha256)
        .bind(file.signing_key_id)
        .bind(file.signature_kid)
        .bind(file.signature)
        .bind(file.signature_alg)
        .bind(file.metadata)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_file_by_id(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        file_id: Uuid,
    ) -> Result<Option<ReleaseFile>, AppError> {
        sqlx::query_as::<_, ReleaseFile>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              storage_key,
              file_name,
              file_size,
              sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              metadata,
              created_at
            from release_files
            where tenant_id = $1
              and app_id = $2
              and id = $3
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(file_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        query: &ReleaseListQuery,
    ) -> Result<Vec<Release>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;
        let status = query
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let include_history = query.include_history.unwrap_or(false);

        sqlx::query_as::<_, Release>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              file_id,
              version,
              version_code,
              status,
              changelog,
              force_update,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              published_at,
              deprecated_at,
              created_at,
              updated_at,
              deleted_at
            from releases
            where tenant_id = $1
              and app_id = $2
              and deleted_at is null
              and ($3::text is null or status = $3)
              and ($3::text is not null or $4::bool or status not in ('deprecated', 'revoked'))
            order by created_at desc, id
            limit $5 offset $6
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(status)
        .bind(include_history)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn create(&self, release: NewRelease) -> Result<Release, AppError> {
        sqlx::query_as::<_, Release>(
            r#"
            insert into releases (
              id,
              tenant_id,
              app_id,
              file_id,
              version,
              version_code,
              changelog,
              force_update
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8)
            returning
              id,
              tenant_id,
              app_id,
              file_id,
              version,
              version_code,
              status,
              changelog,
              force_update,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              published_at,
              deprecated_at,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(release.id)
        .bind(release.tenant_id)
        .bind(release.app_id)
        .bind(release.file_id)
        .bind(release.version)
        .bind(release.version_code)
        .bind(release.changelog)
        .bind(release.force_update)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        release_id: Uuid,
    ) -> Result<Option<Release>, AppError> {
        sqlx::query_as::<_, Release>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              file_id,
              version,
              version_code,
              status,
              changelog,
              force_update,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              published_at,
              deprecated_at,
              created_at,
              updated_at,
              deleted_at
            from releases
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(release_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn publish(
        &self,
        tenant_id: Uuid,
        release_id: Uuid,
    ) -> Result<Option<Release>, AppError> {
        sqlx::query_as::<_, Release>(
            r#"
            update releases
            set
              status = 'published',
              published_at = coalesce(published_at, now()),
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and status = 'draft'
              and deleted_at is null
            returning
              id,
              tenant_id,
              app_id,
              file_id,
              version,
              version_code,
              status,
              changelog,
              force_update,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              published_at,
              deprecated_at,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(release_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn deprecate(
        &self,
        tenant_id: Uuid,
        release_id: Uuid,
    ) -> Result<Option<Release>, AppError> {
        sqlx::query_as::<_, Release>(
            r#"
            update releases
            set
              status = 'deprecated',
              deprecated_at = coalesce(deprecated_at, now()),
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and status = 'published'
              and deleted_at is null
            returning
              id,
              tenant_id,
              app_id,
              file_id,
              version,
              version_code,
              status,
              changelog,
              force_update,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              published_at,
              deprecated_at,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(release_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn latest_published(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
    ) -> Result<Option<ReleaseWithFile>, AppError> {
        sqlx::query_as::<_, ReleaseWithFile>(
            r#"
            select
              r.id,
              r.tenant_id,
              r.app_id,
              r.file_id,
              r.version,
              r.version_code,
              r.status,
              r.changelog,
              r.force_update,
              r.signing_key_id,
              r.signature_kid as release_signature_kid,
              r.signature as release_signature,
              r.signature_alg as release_signature_alg,
              r.published_at,
              r.deprecated_at,
              r.created_at,
              r.updated_at,
              f.file_name,
              f.file_size,
              f.sha256,
              f.signature_kid,
              f.signature,
              f.signature_alg
            from releases r
            join release_files f
              on f.id = r.file_id
             and f.tenant_id = r.tenant_id
             and f.app_id = r.app_id
            where r.tenant_id = $1
              and r.app_id = $2
              and r.status = 'published'
              and r.published_at is not null
              and r.signing_key_id is not null
              and r.signature_kid is not null
              and r.signature is not null
              and r.signature_alg is not null
              and r.deleted_at is null
            order by r.version_code desc, r.published_at desc nulls last, r.created_at desc
            limit 1
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn create_download_token(
        &self,
        token: NewDownloadToken,
    ) -> Result<DownloadToken, AppError> {
        sqlx::query_as::<_, DownloadToken>(
            r#"
            insert into download_tokens (
              id,
              tenant_id,
              app_id,
              device_id,
              file_id,
              token_hash,
              kind,
              expires_at
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8)
            returning
              id,
              tenant_id,
              app_id,
              device_id,
              file_id,
              token_hash,
              kind,
              expires_at,
              used_at,
              revoked_at,
              created_at
            "#,
        )
        .bind(token.id)
        .bind(token.tenant_id)
        .bind(token.app_id)
        .bind(token.device_id)
        .bind(token.file_id)
        .bind(token.token_hash)
        .bind(token.kind)
        .bind(token.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn consume_download_token(
        &self,
        file_name: &str,
        token_hash: &str,
    ) -> Result<Option<ValidatedDownload>, AppError> {
        sqlx::query_as::<_, ValidatedDownload>(
            r#"
            update download_tokens dt
            set used_at = now()
            from release_files f,
                 applications a,
                 devices d
            where dt.token_hash = $1
              and dt.kind = 'release_file'
              and dt.used_at is null
              and dt.revoked_at is null
              and dt.expires_at > now()
              and f.id = dt.file_id
              and f.tenant_id = dt.tenant_id
              and f.app_id = dt.app_id
              and f.file_name = $2
              and a.id = dt.app_id
              and a.tenant_id = dt.tenant_id
              and a.status = 'active'
              and a.deleted_at is null
              and d.id = dt.device_id
              and d.tenant_id = dt.tenant_id
              and d.app_id = dt.app_id
              and d.status = 'active'
              and d.deleted_at is null
              and (
                exists (
                  select 1
                  from licenses l
                  where l.id = d.license_id
                    and l.tenant_id = dt.tenant_id
                    and l.app_id = dt.app_id
                    and l.status = 'active'
                    and l.revoked_at is null
                    and l.deleted_at is null
                    and (l.starts_at is null or l.starts_at <= now())
                    and (l.expires_at is null or l.expires_at > now())
                )
                or exists (
                  select 1
                  from subscriptions s
                  where s.id = d.subscription_id
                    and s.tenant_id = dt.tenant_id
                    and s.app_id = dt.app_id
                    and s.status in ('active', 'trialing')
                    and s.cancelled_at is null
                    and s.deleted_at is null
                    and s.starts_at <= now()
                    and (s.expires_at is null or s.expires_at > now())
                )
              )
            returning
              f.id as file_id,
              dt.device_id,
              f.storage_key,
              f.file_name,
              f.file_size,
              f.sha256
            "#,
        )
        .bind(token_hash)
        .bind(file_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

pub async fn create_release_file_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    file: NewReleaseFile,
) -> Result<ReleaseFile, AppError> {
    sqlx::query_as::<_, ReleaseFile>(
        r#"
        insert into release_files (
          id,
          tenant_id,
          app_id,
          storage_key,
          file_name,
          file_size,
          sha256,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          metadata
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        returning
          id,
          tenant_id,
          app_id,
          storage_key,
          file_name,
          file_size,
          sha256,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          metadata,
          created_at
        "#,
    )
    .bind(file.id)
    .bind(file.tenant_id)
    .bind(file.app_id)
    .bind(file.storage_key)
    .bind(file.file_name)
    .bind(file.file_size)
    .bind(file.sha256)
    .bind(file.signing_key_id)
    .bind(file.signature_kid)
    .bind(file.signature)
    .bind(file.signature_alg)
    .bind(file.metadata)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn create_release_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    release: NewRelease,
) -> Result<Release, AppError> {
    sqlx::query_as::<_, Release>(
        r#"
        insert into releases (
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          changelog,
          force_update
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8)
        returning
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          status,
          changelog,
          force_update,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          published_at,
          deprecated_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(release.id)
    .bind(release.tenant_id)
    .bind(release.app_id)
    .bind(release.file_id)
    .bind(release.version)
    .bind(release.version_code)
    .bind(release.changelog)
    .bind(release.force_update)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn publish_release_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    release_id: Uuid,
) -> Result<Option<Release>, AppError> {
    sqlx::query_as::<_, Release>(
        r#"
        update releases
        set
          status = 'published',
          published_at = coalesce(published_at, now()),
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = 'draft'
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          status,
          changelog,
          force_update,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          published_at,
          deprecated_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(release_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn sign_release_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    release_id: Uuid,
    signing_key_id: Uuid,
    signature_kid: String,
    signature: String,
) -> Result<Option<Release>, AppError> {
    sqlx::query_as::<_, Release>(
        r#"
        update releases
        set
          signing_key_id = $3,
          signature_kid = $4,
          signature = $5,
          signature_alg = 'Ed25519',
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = 'published'
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          status,
          changelog,
          force_update,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          published_at,
          deprecated_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(release_id)
    .bind(signing_key_id)
    .bind(signature_kid)
    .bind(signature)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn update_release_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    release_id: Uuid,
    input: UpdateRelease,
) -> Result<Option<Release>, AppError> {
    sqlx::query_as::<_, Release>(
        r#"
        update releases
        set
          version = $3,
          version_code = $4,
          changelog = $5,
          force_update = $6,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = 'draft'
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          status,
          changelog,
          force_update,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          published_at,
          deprecated_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(release_id)
    .bind(input.version)
    .bind(input.version_code)
    .bind(input.changelog)
    .bind(input.force_update)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn deprecate_release_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    release_id: Uuid,
) -> Result<Option<Release>, AppError> {
    sqlx::query_as::<_, Release>(
        r#"
        update releases
        set
          status = 'deprecated',
          deprecated_at = coalesce(deprecated_at, now()),
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = 'published'
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          status,
          changelog,
          force_update,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          published_at,
          deprecated_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(release_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn delete_draft_release_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    release_id: Uuid,
) -> Result<Option<Release>, AppError> {
    sqlx::query_as::<_, Release>(
        r#"
        update releases
        set
          deleted_at = now(),
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = 'draft'
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          file_id,
          version,
          version_code,
          status,
          changelog,
          force_update,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          published_at,
          deprecated_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(release_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.code().as_deref() == Some("23505") {
            return AppError::conflict("release already exists");
        }
    }

    AppError::dependency(format!("release repository database error: {error}"))
}
