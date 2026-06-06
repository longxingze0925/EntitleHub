use redis::Client;
use tokio::time::timeout;

use crate::{config::RedisConfig, error::AppError, metrics};

pub async fn connect(config: &RedisConfig) -> Result<Client, AppError> {
    let client = Client::open(config.url.as_str()).map_err(|error| {
        metrics::record_redis_error();
        AppError::dependency(format!("redis configuration failed: {error}"))
    })?;

    ping(&client, config.connect_timeout).await?;

    Ok(client)
}

pub async fn ping(client: &Client, timeout_duration: std::time::Duration) -> Result<(), AppError> {
    let mut connection = timeout(timeout_duration, client.get_multiplexed_async_connection())
        .await
        .map_err(|_| {
            metrics::record_redis_error();
            AppError::dependency("redis connection timed out")
        })?
        .map_err(|error| {
            metrics::record_redis_error();
            AppError::dependency(format!("redis connection failed: {error}"))
        })?;

    timeout(
        timeout_duration,
        redis::cmd("PING").query_async::<String>(&mut connection),
    )
    .await
    .map_err(|_| {
        metrics::record_redis_error();
        AppError::dependency("redis ping timed out")
    })?
    .map(|_| ())
    .map_err(|error| {
        metrics::record_redis_error();
        AppError::dependency(format!("redis ping failed: {error}"))
    })
}
