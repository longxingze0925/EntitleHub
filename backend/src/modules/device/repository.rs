use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::device::model::{Device, DeviceKey, DeviceListQuery, NewDevice, NewDeviceKey},
};

#[derive(Clone)]
pub struct DeviceRepository {
    pool: PgPool,
}

impl DeviceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_machine_id(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        machine_id: &str,
    ) -> Result<Option<Device>, AppError> {
        sqlx::query_as::<_, Device>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              status,
              first_seen_at,
              last_seen_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from devices
            where tenant_id = $1
              and app_id = $2
              and machine_id = $3
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(machine_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Device>, AppError> {
        sqlx::query_as::<_, Device>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              status,
              first_seen_at,
              last_seen_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from devices
            where tenant_id = $1
              and app_id = $2
              and id = $3
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id_for_admin(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Device>, AppError> {
        sqlx::query_as::<_, Device>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              status,
              first_seen_at,
              last_seen_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from devices
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

    pub async fn list(
        &self,
        tenant_id: Uuid,
        query: &DeviceListQuery,
    ) -> Result<Vec<Device>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;
        let status = query
            .status
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let machine_id = query
            .machine_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{value}%"));
        let include_history = query.include_history.unwrap_or(false);

        sqlx::query_as::<_, Device>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              status,
              first_seen_at,
              last_seen_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            from devices
            where tenant_id = $1
              and deleted_at is null
              and ($2::uuid is null or app_id = $2)
              and ($3::uuid is null or customer_id = $3)
              and ($4::uuid is null or license_id = $4)
              and ($5::uuid is null or subscription_id = $5)
              and ($6::text is null or status = $6)
              and ($6::text is not null or $8::bool or status <> 'unbound')
              and ($7::text is null or machine_id ilike $7)
            order by updated_at desc, id
            limit $9 offset $10
            "#,
        )
        .bind(tenant_id)
        .bind(query.app_id)
        .bind(query.customer_id)
        .bind(query.license_id)
        .bind(query.subscription_id)
        .bind(status)
        .bind(machine_id)
        .bind(include_history)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn create(&self, device: NewDevice) -> Result<Device, AppError> {
        sqlx::query_as::<_, Device>(
            r#"
            insert into devices (
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              metadata
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              status,
              first_seen_at,
              last_seen_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(device.id)
        .bind(device.tenant_id)
        .bind(device.app_id)
        .bind(device.customer_id)
        .bind(device.license_id)
        .bind(device.subscription_id)
        .bind(device.machine_id)
        .bind(device.device_name)
        .bind(device.os)
        .bind(device.app_version)
        .bind(device.metadata)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn active_count_for_license(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        license_id: Uuid,
    ) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>(
            r#"
            select count(*)
            from devices
            where tenant_id = $1
              and app_id = $2
              and license_id = $3
              and status in ('active', 'disabled')
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(license_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn active_count_for_subscription(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        subscription_id: Uuid,
    ) -> Result<i64, AppError> {
        sqlx::query_scalar::<_, i64>(
            r#"
            select count(*)
            from devices
            where tenant_id = $1
              and app_id = $2
              and subscription_id = $3
              and status in ('active', 'disabled')
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(subscription_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn set_subscription_binding(
        &self,
        tenant_id: Uuid,
        device_id: Uuid,
        customer_id: Uuid,
        subscription_id: Uuid,
    ) -> Result<Option<Device>, AppError> {
        sqlx::query_as::<_, Device>(
            r#"
            update devices
            set
              customer_id = $3,
              license_id = null,
              subscription_id = $4,
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            returning
              id,
              tenant_id,
              app_id,
              customer_id,
              license_id,
              subscription_id,
              machine_id,
              device_name,
              os,
              app_version,
              status,
              first_seen_at,
              last_seen_at,
              metadata,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(device_id)
        .bind(customer_id)
        .bind(subscription_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn is_blacklisted(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        machine_id: &str,
    ) -> Result<bool, AppError> {
        sqlx::query_scalar::<_, bool>(
            r#"
            select exists (
              select 1
              from device_blacklist
              where tenant_id = $1
                and machine_id = $3
                and (app_id is null or app_id = $2)
            )
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(machine_id)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn touch_device(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        device_id: Uuid,
        app_version: Option<String>,
    ) -> Result<(), AppError> {
        sqlx::query(
            r#"
            update devices
            set
              last_seen_at = now(),
              app_version = coalesce($4, app_version),
              updated_at = now()
            where tenant_id = $1
              and app_id = $2
              and id = $3
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(device_id)
        .bind(app_version)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(map_db_error)
    }

    pub async fn find_active_key(
        &self,
        tenant_id: Uuid,
        app_id: Uuid,
        device_id: Uuid,
        key_id: Uuid,
    ) -> Result<Option<DeviceKey>, AppError> {
        sqlx::query_as::<_, DeviceKey>(
            r#"
            select
              id,
              tenant_id,
              app_id,
              device_id,
              public_key,
              algorithm,
              status,
              created_at,
              rotated_at,
              revoked_at
            from device_keys
            where tenant_id = $1
              and app_id = $2
              and device_id = $3
              and id = $4
              and status = 'active'
            "#,
        )
        .bind(tenant_id)
        .bind(app_id)
        .bind(device_id)
        .bind(key_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

pub async fn find_device_by_machine_id_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    machine_id: &str,
) -> Result<Option<Device>, AppError> {
    sqlx::query_as::<_, Device>(
        r#"
        select
          id,
          tenant_id,
          app_id,
          customer_id,
          license_id,
          subscription_id,
          machine_id,
          device_name,
          os,
          app_version,
          status,
          first_seen_at,
          last_seen_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        from devices
        where tenant_id = $1
          and app_id = $2
          and machine_id = $3
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(machine_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn create_device_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    device: NewDevice,
) -> Result<Device, AppError> {
    sqlx::query_as::<_, Device>(
        r#"
        insert into devices (
          id,
          tenant_id,
          app_id,
          customer_id,
          license_id,
          subscription_id,
          machine_id,
          device_name,
          os,
          app_version,
          metadata
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          license_id,
          subscription_id,
          machine_id,
          device_name,
          os,
          app_version,
          status,
          first_seen_at,
          last_seen_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(device.id)
    .bind(device.tenant_id)
    .bind(device.app_id)
    .bind(device.customer_id)
    .bind(device.license_id)
    .bind(device.subscription_id)
    .bind(device.machine_id)
    .bind(device.device_name)
    .bind(device.os)
    .bind(device.app_version)
    .bind(device.metadata)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn active_device_count_for_license_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    license_id: Uuid,
) -> Result<i64, AppError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from devices
        where tenant_id = $1
          and app_id = $2
          and license_id = $3
          and status in ('active', 'disabled')
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(license_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn active_device_count_for_subscription_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    subscription_id: Uuid,
) -> Result<i64, AppError> {
    sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from devices
        where tenant_id = $1
          and app_id = $2
          and subscription_id = $3
          and status in ('active', 'disabled')
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(subscription_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn set_subscription_binding_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    device_id: Uuid,
    customer_id: Uuid,
    subscription_id: Uuid,
) -> Result<Option<Device>, AppError> {
    sqlx::query_as::<_, Device>(
        r#"
        update devices
        set
          customer_id = $3,
          license_id = null,
          subscription_id = $4,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          license_id,
          subscription_id,
          machine_id,
          device_name,
          os,
          app_version,
          status,
          first_seen_at,
          last_seen_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(device_id)
    .bind(customer_id)
    .bind(subscription_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn is_device_blacklisted_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    machine_id: &str,
) -> Result<bool, AppError> {
    sqlx::query_scalar::<_, bool>(
        r#"
        select exists (
          select 1
          from device_blacklist
          where tenant_id = $1
            and machine_id = $3
            and (app_id is null or app_id = $2)
        )
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(machine_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn create_device_key_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    key: NewDeviceKey,
) -> Result<DeviceKey, AppError> {
    sqlx::query_as::<_, DeviceKey>(
        r#"
        insert into device_keys (
          id,
          tenant_id,
          app_id,
          device_id,
          public_key
        )
        values ($1, $2, $3, $4, $5)
        returning
          id,
          tenant_id,
          app_id,
          device_id,
          public_key,
          algorithm,
          status,
          created_at,
          rotated_at,
          revoked_at
        "#,
    )
    .bind(key.id)
    .bind(key.tenant_id)
    .bind(key.app_id)
    .bind(key.device_id)
    .bind(key.public_key)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn rotate_device_key_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    device_id: Uuid,
    key_id: Uuid,
) -> Result<Option<DeviceKey>, AppError> {
    sqlx::query_as::<_, DeviceKey>(
        r#"
        update device_keys
        set
          status = 'rotated',
          rotated_at = now()
        where tenant_id = $1
          and app_id = $2
          and device_id = $3
          and id = $4
          and status = 'active'
        returning
          id,
          tenant_id,
          app_id,
          device_id,
          public_key,
          algorithm,
          status,
          created_at,
          rotated_at,
          revoked_at
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(device_id)
    .bind(key_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn set_device_status_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    device_id: Uuid,
    status: &'static str,
) -> Result<Option<Device>, AppError> {
    sqlx::query_as::<_, Device>(
        r#"
        update devices
        set
          status = $3,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          tenant_id,
          app_id,
          customer_id,
          license_id,
          subscription_id,
          machine_id,
          device_name,
          os,
          app_version,
          status,
          first_seen_at,
          last_seen_at,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(device_id)
    .bind(status)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn add_device_blacklist_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    machine_id: &str,
    reason: &str,
    created_by: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        delete from device_blacklist
        where tenant_id = $1
          and app_id = $2
          and machine_id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(machine_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    sqlx::query(
        r#"
        insert into device_blacklist (
          id,
          tenant_id,
          app_id,
          machine_id,
          reason,
          created_by
        )
        values (gen_random_uuid(), $1, $2, $3, $4, $5)
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(machine_id)
    .bind(reason)
    .bind(created_by)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

pub async fn remove_device_blacklist_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    app_id: Uuid,
    machine_id: &str,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        delete from device_blacklist
        where tenant_id = $1
          and app_id = $2
          and machine_id = $3
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(machine_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

pub async fn revoke_device_sessions_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    device_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_sessions
        set revoked_at = now()
        where tenant_id = $1
          and device_id = $2
          and revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(device_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

pub async fn revoke_device_refresh_tokens_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    device_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_refresh_tokens rt
        set revoked_at = now()
        from client_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and s.device_id = $2
          and rt.revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(device_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("device repository database error: {error}"))
}
