use std::env;

use sqlx::postgres::PgPoolOptions;
use user_admin_backend::{
    crypto::signing::generate_ed25519_key,
    modules::device::{
        model::NewDeviceKey,
        repository::{create_device_key_in_transaction, rotate_device_key_in_transaction},
    },
};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL DATABASE_URL; run with cargo test --test device_key_rotation -- --ignored"]
async fn rotate_device_key_marks_current_key_only_and_leaves_new_key_active(
) -> Result<(), Box<dyn std::error::Error>> {
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must point to a disposable PostgreSQL test database");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let tenant_id = Uuid::new_v4();
    let app_id = Uuid::new_v4();
    let device_id = Uuid::new_v4();
    let mut transaction = pool.begin().await?;

    sqlx::query("insert into tenants (id, name, slug) values ($1, $2, $3)")
        .bind(tenant_id)
        .bind("Device Key Rotation Test")
        .bind(format!("device-key-rotation-{tenant_id}"))
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        r#"
        insert into applications (
          id,
          tenant_id,
          name,
          app_key,
          app_secret_hash
        )
        values ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(app_id)
    .bind(tenant_id)
    .bind("Rotation Test App")
    .bind(format!("app_{app_id}"))
    .bind("secret-hash")
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        r#"
        insert into devices (
          id,
          tenant_id,
          app_id,
          machine_id
        )
        values ($1, $2, $3, $4)
        "#,
    )
    .bind(device_id)
    .bind(tenant_id)
    .bind(app_id)
    .bind(format!("machine-{device_id}"))
    .execute(&mut *transaction)
    .await?;

    let old_key = generate_ed25519_key()?;
    let old_device_key = create_device_key_in_transaction(
        &mut transaction,
        NewDeviceKey::new(tenant_id, app_id, device_id, old_key.public_key_pem)?,
    )
    .await?;
    let rotated_key = rotate_device_key_in_transaction(
        &mut transaction,
        tenant_id,
        app_id,
        device_id,
        old_device_key.id,
    )
    .await?
    .expect("old active key should rotate");

    assert_eq!(rotated_key.id, old_device_key.id);
    assert_eq!(rotated_key.status, "rotated");
    assert!(rotated_key.rotated_at.is_some());

    let stale_rotate = rotate_device_key_in_transaction(
        &mut transaction,
        tenant_id,
        app_id,
        device_id,
        old_device_key.id,
    )
    .await?;
    assert!(
        stale_rotate.is_none(),
        "rotating the same stale key twice should be a no-op"
    );

    let new_key = generate_ed25519_key()?;
    let new_device_key = create_device_key_in_transaction(
        &mut transaction,
        NewDeviceKey::new(tenant_id, app_id, device_id, new_key.public_key_pem)?,
    )
    .await?;
    let active_keys = sqlx::query_as::<_, (Uuid, String)>(
        r#"
        select id, status
        from device_keys
        where tenant_id = $1
          and app_id = $2
          and device_id = $3
          and status = 'active'
        "#,
    )
    .bind(tenant_id)
    .bind(app_id)
    .bind(device_id)
    .fetch_all(&mut *transaction)
    .await?;

    assert_eq!(active_keys, vec![(new_device_key.id, "active".to_owned())]);

    transaction.rollback().await?;

    Ok(())
}
