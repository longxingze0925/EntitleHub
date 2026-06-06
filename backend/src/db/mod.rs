use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::time::timeout;

use crate::{config::DatabaseConfig, error::AppError};

pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, AppError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .connect(&config.url);

    let pool = timeout(config.connect_timeout, pool)
        .await
        .map_err(|_| AppError::dependency("database connection timed out"))?
        .map_err(|error| AppError::dependency(format!("database connection failed: {error}")))?;

    ping(&pool, config.connect_timeout).await?;

    Ok(pool)
}

pub async fn ping(pool: &PgPool, timeout_duration: std::time::Duration) -> Result<(), AppError> {
    timeout(timeout_duration, sqlx::query("select 1").execute(pool))
        .await
        .map_err(|_| AppError::dependency("database ping timed out"))?
        .map(|_| ())
        .map_err(|error| AppError::dependency(format!("database ping failed: {error}")))
}
