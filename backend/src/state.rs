use std::sync::Arc;

use redis::Client as RedisClient;
use sqlx::PgPool;

use crate::{cache, config::AppConfig, db, error::AppError, storage};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub db: PgPool,
    pub redis: RedisClient,
    pub object_store: Arc<dyn storage::ObjectStore>,
}

impl AppState {
    pub async fn connect(config: AppConfig) -> Result<Self, AppError> {
        let db = db::connect(&config.database).await?;
        let redis = cache::connect(&config.redis).await?;
        let object_store = storage::build_object_store(&config.object_storage)?;

        Ok(Self {
            config: Arc::new(config),
            db,
            redis,
            object_store,
        })
    }
}
