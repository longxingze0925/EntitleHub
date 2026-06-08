use sqlx::{PgPool, Postgres, Transaction};

use crate::{
    error::AppError,
    modules::{
        device::{
            model::{normalize_machine_id, Device, DeviceBindInput, NewDevice},
            repository::{
                active_device_count_for_license_in_transaction,
                active_device_count_for_subscription_in_transaction, create_device_in_transaction,
                find_device_by_machine_id_in_transaction, is_device_blacklisted_in_transaction,
                set_subscription_binding_in_transaction, DeviceRepository,
            },
        },
        license::model::License,
        subscription::model::Subscription,
    },
};

#[derive(Clone)]
pub struct DeviceService {
    repository: DeviceRepository,
}

#[derive(Debug, Clone)]
pub struct BindDeviceResult {
    pub device: Device,
    pub created: bool,
}

impl DeviceService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            repository: DeviceRepository::new(pool),
        }
    }

    pub async fn bind_for_license(
        &self,
        input: DeviceBindInput,
        license: &License,
    ) -> Result<BindDeviceResult, AppError> {
        let machine_id = normalize_machine_id(&input.machine_id)?;
        if self
            .repository
            .is_blacklisted(input.tenant_id, input.app_id, &machine_id)
            .await?
        {
            return Err(AppError::device_blacklisted());
        }

        if let Some(existing) = self
            .repository
            .find_by_machine_id(input.tenant_id, input.app_id, &machine_id)
            .await?
        {
            ensure_existing_device_usable(&existing)?;

            return Ok(BindDeviceResult {
                device: existing,
                created: false,
            });
        }

        ensure_device_limit_available(&self.repository, &input, license).await?;
        let device = self
            .repository
            .create(NewDevice::from_bind_input(input)?)
            .await?;

        Ok(BindDeviceResult {
            device,
            created: true,
        })
    }

    pub async fn bind_for_subscription(
        &self,
        input: DeviceBindInput,
        subscription: &Subscription,
    ) -> Result<BindDeviceResult, AppError> {
        let machine_id = normalize_machine_id(&input.machine_id)?;
        if self
            .repository
            .is_blacklisted(input.tenant_id, input.app_id, &machine_id)
            .await?
        {
            return Err(AppError::device_blacklisted());
        }

        if let Some(existing) = self
            .repository
            .find_by_machine_id(input.tenant_id, input.app_id, &machine_id)
            .await?
        {
            ensure_existing_device_usable(&existing)?;
            if existing.subscription_id == Some(subscription.id) {
                return Ok(BindDeviceResult {
                    device: existing,
                    created: false,
                });
            }

            ensure_subscription_device_limit_available(&self.repository, &input, subscription)
                .await?;
            let device = self
                .repository
                .set_subscription_binding(
                    input.tenant_id,
                    existing.id,
                    subscription.customer_id,
                    subscription.id,
                )
                .await?
                .ok_or_else(AppError::device_not_found)?;

            return Ok(BindDeviceResult {
                device,
                created: false,
            });
        }

        ensure_subscription_device_limit_available(&self.repository, &input, subscription).await?;
        let device = self
            .repository
            .create(NewDevice::from_bind_input(input)?)
            .await?;

        Ok(BindDeviceResult {
            device,
            created: true,
        })
    }

    pub async fn bind_for_license_in_transaction(
        transaction: &mut Transaction<'_, Postgres>,
        input: DeviceBindInput,
        license: &License,
    ) -> Result<BindDeviceResult, AppError> {
        let machine_id = normalize_machine_id(&input.machine_id)?;
        if is_device_blacklisted_in_transaction(
            transaction,
            input.tenant_id,
            input.app_id,
            &machine_id,
        )
        .await?
        {
            return Err(AppError::device_blacklisted());
        }

        if let Some(existing) = find_device_by_machine_id_in_transaction(
            transaction,
            input.tenant_id,
            input.app_id,
            &machine_id,
        )
        .await?
        {
            ensure_existing_device_usable(&existing)?;

            return Ok(BindDeviceResult {
                device: existing,
                created: false,
            });
        }

        ensure_device_limit_available_in_transaction(transaction, &input, license).await?;
        let device =
            create_device_in_transaction(transaction, NewDevice::from_bind_input(input)?).await?;

        Ok(BindDeviceResult {
            device,
            created: true,
        })
    }

    pub async fn bind_for_subscription_in_transaction(
        transaction: &mut Transaction<'_, Postgres>,
        input: DeviceBindInput,
        subscription: &Subscription,
    ) -> Result<BindDeviceResult, AppError> {
        let machine_id = normalize_machine_id(&input.machine_id)?;
        if is_device_blacklisted_in_transaction(
            transaction,
            input.tenant_id,
            input.app_id,
            &machine_id,
        )
        .await?
        {
            return Err(AppError::device_blacklisted());
        }

        if let Some(existing) = find_device_by_machine_id_in_transaction(
            transaction,
            input.tenant_id,
            input.app_id,
            &machine_id,
        )
        .await?
        {
            ensure_existing_device_usable(&existing)?;
            if existing.subscription_id == Some(subscription.id) {
                return Ok(BindDeviceResult {
                    device: existing,
                    created: false,
                });
            }

            ensure_subscription_device_limit_available_in_transaction(
                transaction,
                &input,
                subscription,
            )
            .await?;
            let device = set_subscription_binding_in_transaction(
                transaction,
                input.tenant_id,
                existing.id,
                subscription.customer_id,
                subscription.id,
            )
            .await?
            .ok_or_else(AppError::device_not_found)?;

            return Ok(BindDeviceResult {
                device,
                created: false,
            });
        }

        ensure_subscription_device_limit_available_in_transaction(
            transaction,
            &input,
            subscription,
        )
        .await?;
        let device =
            create_device_in_transaction(transaction, NewDevice::from_bind_input(input)?).await?;

        Ok(BindDeviceResult {
            device,
            created: true,
        })
    }

    pub async fn bind_for_customer_session_in_transaction(
        transaction: &mut Transaction<'_, Postgres>,
        input: DeviceBindInput,
    ) -> Result<BindDeviceResult, AppError> {
        let machine_id = normalize_machine_id(&input.machine_id)?;
        if is_device_blacklisted_in_transaction(
            transaction,
            input.tenant_id,
            input.app_id,
            &machine_id,
        )
        .await?
        {
            return Err(AppError::device_blacklisted());
        }

        if let Some(existing) = find_device_by_machine_id_in_transaction(
            transaction,
            input.tenant_id,
            input.app_id,
            &machine_id,
        )
        .await?
        {
            ensure_existing_device_usable(&existing)?;

            return Ok(BindDeviceResult {
                device: existing,
                created: false,
            });
        }

        let device =
            create_device_in_transaction(transaction, NewDevice::from_bind_input(input)?).await?;

        Ok(BindDeviceResult {
            device,
            created: true,
        })
    }
}

async fn ensure_device_limit_available(
    repository: &DeviceRepository,
    input: &DeviceBindInput,
    license: &License,
) -> Result<(), AppError> {
    if license.max_devices == 0 {
        return Err(AppError::device_limit_exceeded());
    }

    let Some(license_id) = input.license_id else {
        return Ok(());
    };

    let active_count = repository
        .active_count_for_license(input.tenant_id, input.app_id, license_id)
        .await?;

    if active_count >= i64::from(license.max_devices) {
        return Err(AppError::device_limit_exceeded());
    }

    Ok(())
}

async fn ensure_subscription_device_limit_available(
    repository: &DeviceRepository,
    input: &DeviceBindInput,
    subscription: &Subscription,
) -> Result<(), AppError> {
    if subscription.max_devices == 0 {
        return Err(AppError::device_limit_exceeded());
    }

    let Some(subscription_id) = input.subscription_id else {
        return Ok(());
    };

    let active_count = repository
        .active_count_for_subscription(input.tenant_id, input.app_id, subscription_id)
        .await?;

    if active_count >= i64::from(subscription.max_devices) {
        return Err(AppError::device_limit_exceeded());
    }

    Ok(())
}

async fn ensure_device_limit_available_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    input: &DeviceBindInput,
    license: &License,
) -> Result<(), AppError> {
    if license.max_devices == 0 {
        return Err(AppError::device_limit_exceeded());
    }

    let Some(license_id) = input.license_id else {
        return Ok(());
    };

    let active_count = active_device_count_for_license_in_transaction(
        transaction,
        input.tenant_id,
        input.app_id,
        license_id,
    )
    .await?;

    if active_count >= i64::from(license.max_devices) {
        return Err(AppError::device_limit_exceeded());
    }

    Ok(())
}

async fn ensure_subscription_device_limit_available_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    input: &DeviceBindInput,
    subscription: &Subscription,
) -> Result<(), AppError> {
    if subscription.max_devices == 0 {
        return Err(AppError::device_limit_exceeded());
    }

    let active_count = active_device_count_for_subscription_in_transaction(
        transaction,
        input.tenant_id,
        input.app_id,
        subscription.id,
    )
    .await?;

    if active_count >= i64::from(subscription.max_devices) {
        return Err(AppError::device_limit_exceeded());
    }

    Ok(())
}

fn ensure_existing_device_usable(device: &Device) -> Result<(), AppError> {
    match device.status.as_str() {
        "active" => Ok(()),
        "blacklisted" => Err(AppError::device_blacklisted()),
        "disabled" | "unbound" => Err(AppError::device_not_activated()),
        _ => Err(AppError::invalid_request("device status is invalid")),
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::{
        error::AppError,
        modules::{device::model::Device, license::model::License},
    };

    use super::{ensure_device_limit_available, ensure_existing_device_usable};

    #[test]
    fn active_existing_device_is_usable() {
        let device = test_device("active");

        assert!(ensure_existing_device_usable(&device).is_ok());
    }

    #[test]
    fn blacklisted_existing_device_is_rejected() {
        let device = test_device("blacklisted");

        assert!(matches!(
            ensure_existing_device_usable(&device),
            Err(AppError::DeviceBlacklisted(_))
        ));
    }

    #[test]
    fn inactive_existing_device_is_rejected_as_not_activated() {
        let disabled = test_device("disabled");
        let unbound = test_device("unbound");

        assert!(matches!(
            ensure_existing_device_usable(&disabled),
            Err(AppError::DeviceNotActivated(_))
        ));
        assert!(matches!(
            ensure_existing_device_usable(&unbound),
            Err(AppError::DeviceNotActivated(_))
        ));
    }

    #[tokio::test]
    async fn zero_device_limit_is_rejected_before_counting() {
        let license = test_license(0);

        let result = ensure_device_limit_available(
            &crate::modules::device::repository::DeviceRepository::new(
                sqlx::PgPool::connect_lazy("postgres://localhost/test").expect("pool"),
            ),
            &crate::modules::device::model::DeviceBindInput {
                tenant_id: Uuid::nil(),
                app_id: Uuid::nil(),
                customer_id: None,
                license_id: Some(Uuid::nil()),
                subscription_id: None,
                machine_id: "machine".to_owned(),
                device_name: None,
                os: None,
                app_version: None,
                metadata: None,
            },
            &license,
        )
        .await;

        assert!(result.is_err());
    }

    fn test_device(status: &str) -> Device {
        Device {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: None,
            license_id: None,
            subscription_id: None,
            machine_id: "machine".to_owned(),
            device_name: None,
            os: None,
            app_version: None,
            status: status.to_owned(),
            first_seen_at: Utc::now(),
            last_seen_at: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    fn test_license(max_devices: i32) -> License {
        License {
            id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            app_id: Uuid::nil(),
            customer_id: None,
            license_key_hash: "hash".to_owned(),
            license_type: "standard".to_owned(),
            status: "active".to_owned(),
            max_devices,
            features: serde_json::json!([]),
            starts_at: Some(Utc::now()),
            expires_at: None,
            revoked_at: None,
            metadata: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }
}
