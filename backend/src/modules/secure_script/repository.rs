use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::secure_script::model::{
        NewSecureScript, SecureScript, SecureScriptListQuery, UpdateSecureScriptContent,
    },
};

#[derive(Clone)]
pub struct SecureScriptRepository {
    pool: PgPool,
}

impl SecureScriptRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, script: NewSecureScript) -> Result<SecureScript, AppError> {
        sqlx::query_as::<_, SecureScript>(
            r#"
            insert into secure_scripts (
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            returning
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              status,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at,
              published_at,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(script.id)
        .bind(script.tenant_id)
        .bind(script.app_id)
        .bind(script.name)
        .bind(script.version)
        .bind(script.version_code)
        .bind(script.content_ciphertext)
        .bind(script.content_sha256)
        .bind(script.signing_key_id)
        .bind(script.signature_kid)
        .bind(script.signature)
        .bind(script.signature_alg)
        .bind(script.required_features)
        .bind(script.expires_at)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        script_id: Uuid,
    ) -> Result<Option<SecureScript>, AppError> {
        sqlx::query_as::<_, SecureScript>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              status,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at,
              published_at,
              created_at,
              updated_at,
              deleted_at
            from secure_scripts
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(script_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        query: &SecureScriptListQuery,
    ) -> Result<Vec<SecureScript>, AppError> {
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

        sqlx::query_as::<_, SecureScript>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              status,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at,
              published_at,
              created_at,
              updated_at,
              deleted_at
            from secure_scripts
            where tenant_id = $1
              and app_id = $2
              and deleted_at is null
              and ($3::text is null or status = $3)
              and ($3::text is not null or $4::bool or status <> 'deprecated')
            order by version_code desc, updated_at desc, id
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

    pub async fn update_content(
        &self,
        tenant_id: Uuid,
        script_id: Uuid,
        input: UpdateSecureScriptContent,
    ) -> Result<Option<SecureScript>, AppError> {
        sqlx::query_as::<_, SecureScript>(
            r#"
            update secure_scripts
            set
              version = coalesce($3, version),
              version_code = coalesce($4, version_code),
              status = 'draft',
              content_ciphertext = $5,
              content_sha256 = $6,
              signing_key_id = $7,
              signature_kid = $8,
              signature = $9,
              signature_alg = $10,
              published_at = null,
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            returning
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              status,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at,
              published_at,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(script_id)
        .bind(input.version)
        .bind(input.version_code)
        .bind(input.content_ciphertext)
        .bind(input.content_sha256)
        .bind(input.signing_key_id)
        .bind(input.signature_kid)
        .bind(input.signature)
        .bind(input.signature_alg)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn publish(
        &self,
        tenant_id: Uuid,
        script_id: Uuid,
    ) -> Result<Option<SecureScript>, AppError> {
        self.set_status(tenant_id, script_id, "draft", "published")
            .await
    }

    pub async fn deprecate(
        &self,
        tenant_id: Uuid,
        script_id: Uuid,
    ) -> Result<Option<SecureScript>, AppError> {
        self.set_status(tenant_id, script_id, "published", "deprecated")
            .await
    }

    pub async fn list_published(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
    ) -> Result<Vec<SecureScript>, AppError> {
        sqlx::query_as::<_, SecureScript>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              status,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at,
              published_at,
              created_at,
              updated_at,
              deleted_at
            from secure_scripts
            where tenant_id = $1
              and app_id = $2
              and status = 'published'
              and (expires_at is null or expires_at > now())
              and deleted_at is null
            order by version_code desc, published_at desc nulls last
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    async fn set_status(
        &self,
        tenant_id: Uuid,
        script_id: Uuid,
        expected_status: &'static str,
        status: &'static str,
    ) -> Result<Option<SecureScript>, AppError> {
        sqlx::query_as::<_, SecureScript>(
            r#"
            update secure_scripts
            set
              status = $3,
              published_at = case when $3 = 'published' then coalesce(published_at, now()) else published_at end,
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and status = $4
              and deleted_at is null
            returning
              id,
              tenant_id,
              app_id,
              name,
              version,
              version_code,
              status,
              content_ciphertext,
              content_sha256,
              signing_key_id,
              signature_kid,
              signature,
              signature_alg,
              required_features,
              expires_at,
              published_at,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(script_id)
        .bind(status)
        .bind(expected_status)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

pub async fn create_secure_script_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    script: NewSecureScript,
) -> Result<SecureScript, AppError> {
    sqlx::query_as::<_, SecureScript>(
        r#"
        insert into secure_scripts (
          id,
          tenant_id,
          app_id,
          name,
          version,
          version_code,
          content_ciphertext,
          content_sha256,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          required_features,
          expires_at
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
        returning
          id,
          tenant_id,
          app_id,
          name,
          version,
          version_code,
          status,
          content_ciphertext,
          content_sha256,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          required_features,
          expires_at,
          published_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(script.id)
    .bind(script.tenant_id)
    .bind(script.app_id)
    .bind(script.name)
    .bind(script.version)
    .bind(script.version_code)
    .bind(script.content_ciphertext)
    .bind(script.content_sha256)
    .bind(script.signing_key_id)
    .bind(script.signature_kid)
    .bind(script.signature)
    .bind(script.signature_alg)
    .bind(script.required_features)
    .bind(script.expires_at)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn update_secure_script_content_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    script_id: Uuid,
    input: UpdateSecureScriptContent,
) -> Result<Option<SecureScript>, AppError> {
    sqlx::query_as::<_, SecureScript>(
        r#"
        update secure_scripts
        set
          version = coalesce($3, version),
          version_code = coalesce($4, version_code),
          status = 'draft',
          content_ciphertext = $5,
          content_sha256 = $6,
          signing_key_id = $7,
          signature_kid = $8,
          signature = $9,
          signature_alg = $10,
          published_at = null,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          name,
          version,
          version_code,
          status,
          content_ciphertext,
          content_sha256,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          required_features,
          expires_at,
          published_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(script_id)
    .bind(input.version)
    .bind(input.version_code)
    .bind(input.content_ciphertext)
    .bind(input.content_sha256)
    .bind(input.signing_key_id)
    .bind(input.signature_kid)
    .bind(input.signature)
    .bind(input.signature_alg)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn publish_secure_script_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    script_id: Uuid,
) -> Result<Option<SecureScript>, AppError> {
    set_secure_script_status_in_transaction(transaction, tenant_id, script_id, "draft", "published")
        .await
}

pub async fn deprecate_secure_script_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    script_id: Uuid,
) -> Result<Option<SecureScript>, AppError> {
    set_secure_script_status_in_transaction(
        transaction,
        tenant_id,
        script_id,
        "published",
        "deprecated",
    )
    .await
}

async fn set_secure_script_status_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    script_id: Uuid,
    expected_status: &'static str,
    status: &'static str,
) -> Result<Option<SecureScript>, AppError> {
    sqlx::query_as::<_, SecureScript>(
        r#"
        update secure_scripts
        set
          status = $3,
          published_at = case when $3 = 'published' then coalesce(published_at, now()) else published_at end,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and status = $4
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          name,
          version,
          version_code,
          status,
          content_ciphertext,
          content_sha256,
          signing_key_id,
          signature_kid,
          signature,
          signature_alg,
          required_features,
          expires_at,
          published_at,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(script_id)
    .bind(status)
    .bind(expected_status)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("secure script repository database error: {error}"))
}
