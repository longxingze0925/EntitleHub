use tracing_subscriber::{fmt, EnvFilter};

use crate::config::AppConfig;

pub fn init(config: &AppConfig) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.app.log_level.clone()));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .init();
}
