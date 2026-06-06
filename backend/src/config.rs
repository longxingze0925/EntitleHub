use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine as _,
};

use crate::error::AppError;

const MIN_SECRET_LEN: usize = 32;
const EXAMPLE_SECRET_VALUE: &str = "change-me-at-least-32-random-bytes";
const DEFAULT_DATABASE_PASSWORDS: &[&str] = &["app_password", "postgres", "password"];

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub app: AppSection,
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub email: EmailConfig,
    pub alerting: AlertingConfig,
    pub redis: RedisConfig,
    pub security: SecurityConfig,
    pub object_storage: ObjectStorageConfig,
}

#[derive(Clone, Debug)]
pub struct AppSection {
    pub env: String,
    pub name: String,
    pub log_level: String,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ServerConfig {
    pub host: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub connect_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct EmailConfig {
    pub outbox_worker_enabled: bool,
    pub smtp_host: Option<String>,
    pub smtp_port: u16,
    pub smtp_user: Option<String>,
    pub smtp_password: Option<String>,
    pub smtp_from: Option<String>,
    pub outbox_poll_interval: Duration,
    pub outbox_processing_timeout: Duration,
    pub outbox_batch_size: i64,
    pub outbox_max_attempts: i32,
}

#[derive(Clone, Debug)]
pub struct AlertingConfig {
    pub webhook_token: Option<String>,
    pub delivery_timeout: Duration,
}

impl DatabaseConfig {
    pub fn from_env() -> Result<Self, AppError> {
        Ok(Self {
            url: required_env("DATABASE_URL")?,
            max_connections: parse_u32("DATABASE_MAX_CONNECTIONS", 5)?,
            connect_timeout: Duration::from_secs(parse_u64("DATABASE_CONNECT_TIMEOUT_SECONDS", 5)?),
        })
    }
}

#[derive(Clone, Debug)]
pub struct RedisConfig {
    pub url: String,
    pub connect_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct ObjectStorageConfig {
    pub mode: String,
    pub local_root: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: String,
}

#[derive(Clone, Debug)]
pub struct SecurityConfig {
    pub session_secret: String,
    pub token_hash_pepper: String,
    pub refresh_token_pepper: String,
    pub csrf_secret: String,
    pub master_key: [u8; 32],
    pub jwt_issuer: String,
    pub jwt_audience: String,
    pub cookie_secure: bool,
    pub admin_session_ttl_seconds: i64,
    pub client_access_token_ttl_seconds: i64,
    pub client_refresh_token_ttl_seconds: i64,
    pub client_session_ttl_seconds: i64,
    pub download_token_ttl_seconds: i64,
    pub login_rate_limit_max: u32,
    pub login_rate_limit_window_seconds: u64,
    pub activation_rate_limit_max: u32,
    pub activation_rate_limit_window_seconds: u64,
    pub refresh_rate_limit_max: u32,
    pub refresh_rate_limit_window_seconds: u64,
    pub heartbeat_rate_limit_max: u32,
    pub heartbeat_rate_limit_window_seconds: u64,
    pub client_action_rate_limit_max: u32,
    pub client_action_rate_limit_window_seconds: u64,
    pub download_rate_limit_max: u32,
    pub download_rate_limit_window_seconds: u64,
    pub allowed_origins: Vec<String>,
    pub trusted_proxies: Vec<IpAddr>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let app = AppSection {
            env: env_value("APP_ENV", "development"),
            name: env_value("APP_NAME", "user-admin-backend"),
            log_level: env_value("APP_LOG_LEVEL", "info"),
            base_url: optional_env("APP_BASE_URL"),
        };

        let server = ServerConfig {
            host: parse_host()?,
            port: parse_port()?,
        };

        let database = DatabaseConfig::from_env()?;
        let email = EmailConfig::from_env()?;
        let alerting = AlertingConfig::from_env()?;

        let redis = RedisConfig {
            url: required_env("REDIS_URL")?,
            connect_timeout: Duration::from_secs(parse_u64("REDIS_CONNECT_TIMEOUT_SECONDS", 5)?),
        };

        let allowed_origins = parse_csv_env("ALLOWED_ORIGINS");
        let trusted_proxies = parse_ip_csv_env("TRUSTED_PROXIES")?;

        let security = SecurityConfig {
            session_secret: required_secret_env("SESSION_SECRET")?,
            token_hash_pepper: required_secret_env("TOKEN_HASH_PEPPER")?,
            refresh_token_pepper: required_secret_env("REFRESH_TOKEN_PEPPER")?,
            csrf_secret: required_secret_env("CSRF_SECRET")?,
            master_key: parse_master_key()?,
            jwt_issuer: required_env("JWT_ISSUER")?,
            jwt_audience: required_env("JWT_AUDIENCE")?,
            cookie_secure: parse_bool("COOKIE_SECURE", false)?,
            admin_session_ttl_seconds: parse_i64("ADMIN_SESSION_TTL_SECONDS", 86_400)?,
            client_access_token_ttl_seconds: parse_i64("CLIENT_ACCESS_TOKEN_TTL_SECONDS", 900)?,
            client_refresh_token_ttl_seconds: parse_i64(
                "CLIENT_REFRESH_TOKEN_TTL_SECONDS",
                2_592_000,
            )?,
            client_session_ttl_seconds: parse_i64("CLIENT_SESSION_TTL_SECONDS", 2_592_000)?,
            download_token_ttl_seconds: parse_i64("DOWNLOAD_TOKEN_TTL_SECONDS", 300)?,
            login_rate_limit_max: parse_u32("LOGIN_RATE_LIMIT_MAX", 10)?,
            login_rate_limit_window_seconds: parse_u64("LOGIN_RATE_LIMIT_WINDOW_SECONDS", 300)?,
            activation_rate_limit_max: parse_u32("ACTIVATION_RATE_LIMIT_MAX", 20)?,
            activation_rate_limit_window_seconds: parse_u64(
                "ACTIVATION_RATE_LIMIT_WINDOW_SECONDS",
                300,
            )?,
            refresh_rate_limit_max: parse_u32("REFRESH_RATE_LIMIT_MAX", 60)?,
            refresh_rate_limit_window_seconds: parse_u64("REFRESH_RATE_LIMIT_WINDOW_SECONDS", 300)?,
            heartbeat_rate_limit_max: parse_u32("HEARTBEAT_RATE_LIMIT_MAX", 120)?,
            heartbeat_rate_limit_window_seconds: parse_u64(
                "HEARTBEAT_RATE_LIMIT_WINDOW_SECONDS",
                60,
            )?,
            client_action_rate_limit_max: parse_u32("CLIENT_ACTION_RATE_LIMIT_MAX", 120)?,
            client_action_rate_limit_window_seconds: parse_u64(
                "CLIENT_ACTION_RATE_LIMIT_WINDOW_SECONDS",
                60,
            )?,
            download_rate_limit_max: parse_u32("DOWNLOAD_RATE_LIMIT_MAX", 30)?,
            download_rate_limit_window_seconds: parse_u64(
                "DOWNLOAD_RATE_LIMIT_WINDOW_SECONDS",
                300,
            )?,
            allowed_origins,
            trusted_proxies,
        };
        validate_security_ttls(&security)?;
        let object_storage = ObjectStorageConfig::from_env()?;

        validate_production_config(&app, &database, &redis, &security)?;

        Ok(Self {
            app,
            server,
            database,
            email,
            alerting,
            redis,
            security,
            object_storage,
        })
    }
}

impl EmailConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let outbox_worker_enabled = parse_bool("EMAIL_OUTBOX_WORKER_ENABLED", false)?;
        let config = Self {
            outbox_worker_enabled,
            smtp_host: optional_env("SMTP_HOST"),
            smtp_port: parse_u16("SMTP_PORT", 587)?,
            smtp_user: optional_env("SMTP_USER"),
            smtp_password: optional_env("SMTP_PASSWORD"),
            smtp_from: optional_env("SMTP_FROM"),
            outbox_poll_interval: Duration::from_secs(
                parse_u64("EMAIL_OUTBOX_POLL_INTERVAL_SECONDS", 15)?.max(1),
            ),
            outbox_processing_timeout: Duration::from_secs(
                parse_u64("EMAIL_OUTBOX_PROCESSING_TIMEOUT_SECONDS", 300)?.max(30),
            ),
            outbox_batch_size: parse_i64("EMAIL_OUTBOX_BATCH_SIZE", 10)?.clamp(1, 100),
            outbox_max_attempts: parse_i32("EMAIL_OUTBOX_MAX_ATTEMPTS", 5)?.clamp(1, 20),
        };

        if config.outbox_worker_enabled {
            if config.smtp_host.is_none() {
                return Err(AppError::config(
                    "SMTP_HOST is required when EMAIL_OUTBOX_WORKER_ENABLED=true",
                ));
            }
            if config.smtp_from.is_none() {
                return Err(AppError::config(
                    "SMTP_FROM is required when EMAIL_OUTBOX_WORKER_ENABLED=true",
                ));
            }
            if config.smtp_user.is_some() != config.smtp_password.is_some() {
                return Err(AppError::config(
                    "SMTP_USER and SMTP_PASSWORD must be configured together",
                ));
            }
        }

        Ok(config)
    }
}

impl AlertingConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let webhook_token = optional_env("ALERTMANAGER_WEBHOOK_TOKEN");
        if webhook_token
            .as_deref()
            .is_some_and(|token| token.len() < MIN_SECRET_LEN)
        {
            return Err(AppError::config(format!(
                "ALERTMANAGER_WEBHOOK_TOKEN must be at least {MIN_SECRET_LEN} characters"
            )));
        }

        Ok(Self {
            webhook_token,
            delivery_timeout: Duration::from_secs(
                parse_u64("ALERT_DELIVERY_TIMEOUT_SECONDS", 10)?.clamp(1, 60),
            ),
        })
    }
}

impl ObjectStorageConfig {
    pub fn from_env() -> Result<Self, AppError> {
        let mode = env_value("OBJECT_STORAGE_MODE", "local")
            .trim()
            .to_lowercase();
        if !matches!(mode.as_str(), "local" | "s3") {
            return Err(AppError::config("OBJECT_STORAGE_MODE must be local or s3"));
        }

        let local_root = optional_env("OBJECT_STORAGE_LOCAL_ROOT").map(PathBuf::from);
        if mode == "local" && local_root.is_none() {
            return Err(AppError::config(
                "OBJECT_STORAGE_LOCAL_ROOT is required when OBJECT_STORAGE_MODE=local",
            ));
        }
        let endpoint = optional_env("OBJECT_STORAGE_ENDPOINT");
        let bucket = optional_env("OBJECT_STORAGE_BUCKET");
        let access_key = optional_env("OBJECT_STORAGE_ACCESS_KEY");
        let secret_key = optional_env("OBJECT_STORAGE_SECRET_KEY");
        if mode == "s3" {
            if endpoint.is_none() {
                return Err(AppError::config(
                    "OBJECT_STORAGE_ENDPOINT is required when OBJECT_STORAGE_MODE=s3",
                ));
            }
            if bucket.is_none() {
                return Err(AppError::config(
                    "OBJECT_STORAGE_BUCKET is required when OBJECT_STORAGE_MODE=s3",
                ));
            }
            if access_key.is_none() {
                return Err(AppError::config(
                    "OBJECT_STORAGE_ACCESS_KEY is required when OBJECT_STORAGE_MODE=s3",
                ));
            }
            if secret_key.is_none() {
                return Err(AppError::config(
                    "OBJECT_STORAGE_SECRET_KEY is required when OBJECT_STORAGE_MODE=s3",
                ));
            }
        }
        let region = env_value("OBJECT_STORAGE_REGION", "us-east-1")
            .trim()
            .to_owned();
        if region.is_empty() {
            return Err(AppError::config("OBJECT_STORAGE_REGION must not be empty"));
        }

        Ok(Self {
            mode,
            local_root,
            endpoint,
            bucket,
            access_key,
            secret_key,
            region,
        })
    }
}

impl ServerConfig {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

fn env_value(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_owned())
}

fn required_env(key: &str) -> Result<String, AppError> {
    env::var(key).map_err(|_| AppError::config(format!("{key} is required")))
}

fn required_secret_env(key: &str) -> Result<String, AppError> {
    let value = required_env(key)?.trim().to_owned();
    if value.len() < MIN_SECRET_LEN {
        return Err(AppError::config(format!(
            "{key} must be at least {MIN_SECRET_LEN} characters"
        )));
    }

    Ok(value)
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_csv_env(key: &str) -> Vec<String> {
    optional_env(key)
        .map(|value| parse_csv_value(&value))
        .unwrap_or_default()
}

fn parse_ip_csv_env(key: &str) -> Result<Vec<IpAddr>, AppError> {
    optional_env(key)
        .map(|value| parse_ip_csv_value(key, &value))
        .unwrap_or_else(|| Ok(Vec::new()))
}

fn parse_ip_csv_value(key: &str, value: &str) -> Result<Vec<IpAddr>, AppError> {
    let mut items = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse::<IpAddr>()
                .map_err(|_| AppError::config(format!("{key} must contain valid IP addresses")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    items.sort();
    items.dedup();

    Ok(items)
}

fn parse_csv_value(value: &str) -> Vec<String> {
    let mut items = value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    items.sort();
    items.dedup();

    items
}

fn parse_host() -> Result<IpAddr, AppError> {
    env_value("APP_HOST", "127.0.0.1")
        .parse()
        .map_err(|_| AppError::config("APP_HOST must be a valid IP address"))
}

fn parse_port() -> Result<u16, AppError> {
    env_value("APP_PORT", "8080")
        .parse()
        .map_err(|_| AppError::config("APP_PORT must be a valid u16 port"))
}

fn parse_u32(key: &str, default: u32) -> Result<u32, AppError> {
    env::var(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| AppError::config(format!("{key} must be a valid u32")))
        })
        .unwrap_or(Ok(default))
}

fn parse_u16(key: &str, default: u16) -> Result<u16, AppError> {
    env::var(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| AppError::config(format!("{key} must be a valid u16")))
        })
        .unwrap_or(Ok(default))
}

fn parse_u64(key: &str, default: u64) -> Result<u64, AppError> {
    env::var(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| AppError::config(format!("{key} must be a valid u64")))
        })
        .unwrap_or(Ok(default))
}

fn parse_i64(key: &str, default: i64) -> Result<i64, AppError> {
    env::var(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| AppError::config(format!("{key} must be a valid i64")))
        })
        .unwrap_or(Ok(default))
}

fn parse_i32(key: &str, default: i32) -> Result<i32, AppError> {
    env::var(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| AppError::config(format!("{key} must be a valid i32")))
        })
        .unwrap_or(Ok(default))
}

fn parse_bool(key: &str, default: bool) -> Result<bool, AppError> {
    env::var(key)
        .map(|value| {
            value
                .parse()
                .map_err(|_| AppError::config(format!("{key} must be true or false")))
        })
        .unwrap_or(Ok(default))
}

fn parse_master_key() -> Result<[u8; 32], AppError> {
    let value = required_env("MASTER_KEY")?;
    parse_master_key_value(&value)
}

fn parse_master_key_value(value: &str) -> Result<[u8; 32], AppError> {
    let trimmed = value.trim();
    let decoded = STANDARD
        .decode(trimmed)
        .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed))
        .map_err(|_| AppError::config("MASTER_KEY must be base64 encoded 32 bytes"))?;

    decoded
        .try_into()
        .map_err(|_| AppError::config("MASTER_KEY must decode to exactly 32 bytes"))
}

fn validate_production_config(
    app: &AppSection,
    database: &DatabaseConfig,
    redis: &RedisConfig,
    security: &SecurityConfig,
) -> Result<(), AppError> {
    if app.env != "production" {
        return Ok(());
    }

    if !security.cookie_secure {
        return Err(AppError::config(
            "COOKIE_SECURE must be true when APP_ENV=production",
        ));
    }

    let base_url = app
        .base_url
        .as_deref()
        .ok_or_else(|| AppError::config("APP_BASE_URL is required when APP_ENV=production"))?;
    if !is_https_url(base_url) {
        return Err(AppError::config(
            "APP_BASE_URL must use https when APP_ENV=production",
        ));
    }

    if security.allowed_origins.is_empty() {
        return Err(AppError::config(
            "ALLOWED_ORIGINS is required when APP_ENV=production",
        ));
    }
    if security
        .allowed_origins
        .iter()
        .any(|origin| !is_https_url(origin))
    {
        return Err(AppError::config(
            "ALLOWED_ORIGINS must use https origins when APP_ENV=production",
        ));
    }

    for (key, value) in [
        ("SESSION_SECRET", security.session_secret.as_str()),
        ("CSRF_SECRET", security.csrf_secret.as_str()),
        ("TOKEN_HASH_PEPPER", security.token_hash_pepper.as_str()),
        (
            "REFRESH_TOKEN_PEPPER",
            security.refresh_token_pepper.as_str(),
        ),
    ] {
        if value == EXAMPLE_SECRET_VALUE {
            return Err(AppError::config(format!(
                "{key} must not use the example value"
            )));
        }
    }

    validate_database_url(&database.url)?;
    validate_redis_url(&redis.url)?;

    Ok(())
}

fn validate_security_ttls(security: &SecurityConfig) -> Result<(), AppError> {
    for (key, value) in [
        (
            "ADMIN_SESSION_TTL_SECONDS",
            security.admin_session_ttl_seconds,
        ),
        (
            "CLIENT_ACCESS_TOKEN_TTL_SECONDS",
            security.client_access_token_ttl_seconds,
        ),
        (
            "CLIENT_REFRESH_TOKEN_TTL_SECONDS",
            security.client_refresh_token_ttl_seconds,
        ),
        (
            "CLIENT_SESSION_TTL_SECONDS",
            security.client_session_ttl_seconds,
        ),
        (
            "DOWNLOAD_TOKEN_TTL_SECONDS",
            security.download_token_ttl_seconds,
        ),
    ] {
        if value <= 0 {
            return Err(AppError::config(format!("{key} must be greater than 0")));
        }
    }

    Ok(())
}

fn is_https_url(value: &str) -> bool {
    value
        .trim()
        .strip_prefix("https://")
        .is_some_and(|rest| !rest.trim().is_empty())
}

fn validate_database_url(url: &str) -> Result<(), AppError> {
    let password = extract_url_password(url).ok_or_else(|| {
        AppError::config("DATABASE_URL must include a non-empty password when APP_ENV=production")
    })?;

    if password.trim().is_empty() {
        return Err(AppError::config(
            "DATABASE_URL must include a non-empty password when APP_ENV=production",
        ));
    }

    if DEFAULT_DATABASE_PASSWORDS.contains(&password) {
        return Err(AppError::config(
            "DATABASE_URL must not use a default password when APP_ENV=production",
        ));
    }

    Ok(())
}

fn validate_redis_url(url: &str) -> Result<(), AppError> {
    let password = extract_url_password(url).ok_or_else(|| {
        AppError::config("REDIS_URL must include a non-empty password when APP_ENV=production")
    })?;

    if password.trim().is_empty() {
        return Err(AppError::config(
            "REDIS_URL must include a non-empty password when APP_ENV=production",
        ));
    }

    Ok(())
}

fn extract_url_password(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let authority = rest
        .split(|character| matches!(character, '/' | '?' | '#'))
        .next()
        .unwrap_or(rest);
    let (userinfo, _) = authority.rsplit_once('@')?;
    userinfo.rsplit_once(':').map(|(_, password)| password)
}

#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{
        extract_url_password, optional_env, parse_csv_value, parse_ip_csv_value,
        parse_master_key_value, required_secret_env, validate_production_config,
        validate_security_ttls, AlertingConfig, AppSection, DatabaseConfig, ObjectStorageConfig,
        RedisConfig, SecurityConfig,
    };
    use std::time::Duration;

    #[test]
    fn master_key_accepts_base64_encoded_32_bytes() {
        let encoded = STANDARD.encode([7_u8; 32]);

        let key = parse_master_key_value(&encoded).expect("master key should parse");

        assert_eq!(key, [7_u8; 32]);
    }

    #[test]
    fn master_key_rejects_wrong_length() {
        let encoded = STANDARD.encode([7_u8; 16]);

        assert!(parse_master_key_value(&encoded).is_err());
    }

    #[test]
    fn optional_env_treats_blank_as_none() {
        std::env::set_var("OPTIONAL_ENV_TEST", " ");

        assert_eq!(optional_env("OPTIONAL_ENV_TEST"), None);

        std::env::remove_var("OPTIONAL_ENV_TEST");
    }

    #[test]
    fn required_secret_env_rejects_missing_or_short_values() {
        std::env::remove_var("SECRET_ENV_TEST");
        assert!(required_secret_env("SECRET_ENV_TEST").is_err());

        std::env::set_var("SECRET_ENV_TEST", "short");
        assert!(required_secret_env("SECRET_ENV_TEST").is_err());

        std::env::set_var("SECRET_ENV_TEST", "a".repeat(32));
        assert_eq!(
            required_secret_env("SECRET_ENV_TEST").expect("secret should parse"),
            "a".repeat(32)
        );

        std::env::remove_var("SECRET_ENV_TEST");
    }

    #[test]
    fn csv_env_parser_trims_sorts_and_dedupes() {
        assert_eq!(
            parse_csv_value(
                " https://b.example.com, https://a.example.com ,,https://a.example.com "
            ),
            vec![
                "https://a.example.com".to_owned(),
                "https://b.example.com".to_owned()
            ]
        );
    }

    #[test]
    fn ip_csv_parser_accepts_ipv4_and_ipv6() {
        assert_eq!(
            parse_ip_csv_value("TRUSTED_PROXIES", "127.0.0.1, ::1,127.0.0.1")
                .expect("trusted proxy ips"),
            vec![
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(Ipv6Addr::LOCALHOST)
            ]
        );
    }

    #[test]
    fn ip_csv_parser_rejects_invalid_values() {
        assert!(parse_ip_csv_value("TRUSTED_PROXIES", "not-an-ip").is_err());
    }

    #[test]
    fn url_password_parser_handles_userinfo_variants() {
        assert_eq!(
            extract_url_password("postgres://app_user:strong-password@db/prod"),
            Some("strong-password")
        );
        assert_eq!(
            extract_url_password("redis://:strong-password@redis/0"),
            Some("strong-password")
        );
        assert_eq!(extract_url_password("redis://redis/0"), None);
    }

    #[test]
    fn security_ttls_must_be_positive() {
        let mut security = test_security(true, "session-secret-value-32-bytes-ok");
        validate_security_ttls(&security).expect("default ttl values should pass");

        security.client_access_token_ttl_seconds = 0;
        let error = validate_security_ttls(&security).expect_err("zero ttl should fail");

        assert!(error
            .to_string()
            .contains("CLIENT_ACCESS_TOKEN_TTL_SECONDS"));
    }

    #[test]
    fn object_storage_config_validates_modes_and_s3_required_values() {
        let _env = ObjectStorageEnvSnapshot::capture();

        clear_object_storage_env();
        let error = ObjectStorageConfig::from_env().expect_err("local root should be required");
        assert!(error.to_string().contains("OBJECT_STORAGE_LOCAL_ROOT"));

        std::env::set_var("OBJECT_STORAGE_LOCAL_ROOT", "./storage");
        let config = ObjectStorageConfig::from_env().expect("local object storage");
        assert_eq!(config.mode, "local");
        assert_eq!(config.region, "us-east-1");

        clear_object_storage_env();
        std::env::set_var("OBJECT_STORAGE_MODE", "s3");
        let error = ObjectStorageConfig::from_env().expect_err("s3 endpoint should be required");
        assert!(error.to_string().contains("OBJECT_STORAGE_ENDPOINT"));

        std::env::set_var("OBJECT_STORAGE_ENDPOINT", "http://minio:9000");
        std::env::set_var("OBJECT_STORAGE_BUCKET", "releases");
        std::env::set_var("OBJECT_STORAGE_ACCESS_KEY", "access");
        std::env::set_var("OBJECT_STORAGE_SECRET_KEY", "secret");
        std::env::set_var("OBJECT_STORAGE_REGION", "ap-southeast-1");
        let config = ObjectStorageConfig::from_env().expect("s3 object storage");

        assert_eq!(config.mode, "s3");
        assert_eq!(config.endpoint.as_deref(), Some("http://minio:9000"));
        assert_eq!(config.bucket.as_deref(), Some("releases"));
        assert_eq!(config.region, "ap-southeast-1");
    }

    #[test]
    fn alerting_config_rejects_short_webhook_token() {
        let _env = AlertingEnvSnapshot::capture();

        std::env::set_var("ALERTMANAGER_WEBHOOK_TOKEN", "short");
        assert!(AlertingConfig::from_env().is_err());

        std::env::set_var("ALERTMANAGER_WEBHOOK_TOKEN", "a".repeat(32));
        std::env::set_var("ALERT_DELIVERY_TIMEOUT_SECONDS", "3");
        let config = AlertingConfig::from_env().expect("alerting config");

        assert_eq!(
            config.webhook_token.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(config.delivery_timeout, Duration::from_secs(3));
    }

    #[test]
    fn production_config_rejects_insecure_cookie() {
        let app = test_app(Some("https://api.example.com"));
        let database = test_database("postgres://app_user:strong-password@db/prod");
        let redis = test_redis("redis://:strong-password@redis/0");
        let security = test_security(false, "session-secret-value-32-bytes-ok");

        let error = validate_production_config(&app, &database, &redis, &security)
            .expect_err("insecure production cookie should fail");

        assert!(error.to_string().contains("COOKIE_SECURE"));
    }

    #[test]
    fn production_config_rejects_http_base_url() {
        let app = test_app(Some("http://api.example.com"));
        let database = test_database("postgres://app_user:strong-password@db/prod");
        let redis = test_redis("redis://:strong-password@redis/0");
        let security = test_security(true, "session-secret-value-32-bytes-ok");

        let error = validate_production_config(&app, &database, &redis, &security)
            .expect_err("http base url should fail");

        assert!(error.to_string().contains("APP_BASE_URL"));
    }

    #[test]
    fn production_config_rejects_example_secret_and_default_database_password() {
        let app = test_app(Some("https://api.example.com"));
        let database = test_database("postgres://app_user:strong-password@db/prod");
        let redis = test_redis("redis://:strong-password@redis/0");
        let security = test_security(true, "change-me-at-least-32-random-bytes");

        let error = validate_production_config(&app, &database, &redis, &security)
            .expect_err("example secret should fail");
        assert!(error.to_string().contains("SESSION_SECRET"));

        let database = test_database("postgres://app_user:app_password@db/prod");
        let security = test_security(true, "session-secret-value-32-bytes-ok");
        let error = validate_production_config(&app, &database, &redis, &security)
            .expect_err("default database password should fail");
        assert!(error.to_string().contains("DATABASE_URL"));
    }

    #[test]
    fn production_config_rejects_redis_without_password() {
        let app = test_app(Some("https://api.example.com"));
        let database = test_database("postgres://app_user:strong-password@db/prod");
        let redis = test_redis("redis://redis/0");
        let security = test_security(true, "session-secret-value-32-bytes-ok");

        let error = validate_production_config(&app, &database, &redis, &security)
            .expect_err("redis without password should fail");

        assert!(error.to_string().contains("REDIS_URL"));
    }

    #[test]
    fn production_config_accepts_hardened_values() {
        let app = test_app(Some("https://api.example.com"));
        let database = test_database("postgres://app_user:strong-password@db/prod");
        let redis = test_redis("redis://:strong-password@redis/0");
        let security = test_security(true, "session-secret-value-32-bytes-ok");

        validate_production_config(&app, &database, &redis, &security)
            .expect("hardened production config should pass");
    }

    struct ObjectStorageEnvSnapshot {
        values: Vec<(&'static str, Option<String>)>,
    }

    struct AlertingEnvSnapshot {
        token: Option<String>,
        timeout: Option<String>,
    }

    impl AlertingEnvSnapshot {
        fn capture() -> Self {
            Self {
                token: std::env::var("ALERTMANAGER_WEBHOOK_TOKEN").ok(),
                timeout: std::env::var("ALERT_DELIVERY_TIMEOUT_SECONDS").ok(),
            }
        }
    }

    impl Drop for AlertingEnvSnapshot {
        fn drop(&mut self) {
            if let Some(value) = &self.token {
                std::env::set_var("ALERTMANAGER_WEBHOOK_TOKEN", value);
            } else {
                std::env::remove_var("ALERTMANAGER_WEBHOOK_TOKEN");
            }
            if let Some(value) = &self.timeout {
                std::env::set_var("ALERT_DELIVERY_TIMEOUT_SECONDS", value);
            } else {
                std::env::remove_var("ALERT_DELIVERY_TIMEOUT_SECONDS");
            }
        }
    }

    impl ObjectStorageEnvSnapshot {
        fn capture() -> Self {
            Self {
                values: OBJECT_STORAGE_ENV_KEYS
                    .iter()
                    .map(|key| (*key, std::env::var(key).ok()))
                    .collect(),
            }
        }
    }

    impl Drop for ObjectStorageEnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    const OBJECT_STORAGE_ENV_KEYS: &[&str] = &[
        "OBJECT_STORAGE_MODE",
        "OBJECT_STORAGE_LOCAL_ROOT",
        "OBJECT_STORAGE_ENDPOINT",
        "OBJECT_STORAGE_BUCKET",
        "OBJECT_STORAGE_ACCESS_KEY",
        "OBJECT_STORAGE_SECRET_KEY",
        "OBJECT_STORAGE_REGION",
    ];

    fn clear_object_storage_env() {
        for key in OBJECT_STORAGE_ENV_KEYS {
            std::env::remove_var(key);
        }
    }

    fn test_app(base_url: Option<&str>) -> AppSection {
        AppSection {
            env: "production".to_owned(),
            name: "user-admin-backend".to_owned(),
            log_level: "info".to_owned(),
            base_url: base_url.map(str::to_owned),
        }
    }

    fn test_database(url: &str) -> DatabaseConfig {
        DatabaseConfig {
            url: url.to_owned(),
            max_connections: 5,
            connect_timeout: Duration::from_secs(5),
        }
    }

    fn test_redis(url: &str) -> RedisConfig {
        RedisConfig {
            url: url.to_owned(),
            connect_timeout: Duration::from_secs(5),
        }
    }

    fn test_security(cookie_secure: bool, session_secret: &str) -> SecurityConfig {
        SecurityConfig {
            session_secret: session_secret.to_owned(),
            token_hash_pepper: "token-hash-pepper-value-32-bytes".to_owned(),
            refresh_token_pepper: "refresh-token-pepper-value-32-bytes".to_owned(),
            csrf_secret: "csrf-secret-value-32-bytes-long".to_owned(),
            master_key: [1_u8; 32],
            jwt_issuer: "https://api.example.com".to_owned(),
            jwt_audience: "client-sdk".to_owned(),
            cookie_secure,
            admin_session_ttl_seconds: 86_400,
            client_access_token_ttl_seconds: 900,
            client_refresh_token_ttl_seconds: 2_592_000,
            client_session_ttl_seconds: 2_592_000,
            download_token_ttl_seconds: 300,
            login_rate_limit_max: 10,
            login_rate_limit_window_seconds: 300,
            activation_rate_limit_max: 20,
            activation_rate_limit_window_seconds: 300,
            refresh_rate_limit_max: 60,
            refresh_rate_limit_window_seconds: 300,
            heartbeat_rate_limit_max: 120,
            heartbeat_rate_limit_window_seconds: 60,
            client_action_rate_limit_max: 120,
            client_action_rate_limit_window_seconds: 60,
            download_rate_limit_max: 30,
            download_rate_limit_window_seconds: 300,
            allowed_origins: vec!["https://admin.example.com".to_owned()],
            trusted_proxies: Vec::new(),
        }
    }
}
