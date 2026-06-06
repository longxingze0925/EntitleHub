use chrono::{DateTime, Utc};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct ClientSession {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub device_id: Uuid,
    pub machine_id: String,
    pub auth_mode: String,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewClientSession {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub app_id: Uuid,
    pub customer_id: Option<Uuid>,
    pub device_id: Uuid,
    pub machine_id: String,
    pub auth_mode: String,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ClientRefreshToken {
    pub id: Uuid,
    pub session_id: Uuid,
    pub token_hash: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewClientRefreshToken {
    pub id: Uuid,
    pub session_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

impl NewClientSession {
    pub fn new(
        tenant_id: Uuid,
        app_id: Uuid,
        customer_id: Option<Uuid>,
        device_id: Uuid,
        machine_id: String,
        auth_mode: String,
        expires_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            app_id,
            customer_id,
            device_id,
            machine_id,
            auth_mode,
            user_agent: None,
            client_ip: None,
            expires_at,
        }
    }
}

impl NewClientRefreshToken {
    pub fn new(session_id: Uuid, token_hash: String, expires_at: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            session_id,
            token_hash,
            expires_at,
        }
    }
}
