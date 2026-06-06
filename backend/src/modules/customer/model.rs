use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct Customer {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub password_hash: Option<String>,
    pub name: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub status: String,
    pub email_verified: bool,
    pub metadata: Value,
    pub remark: Option<String>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub last_login_ip: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewCustomer {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub password_hash: Option<String>,
    pub name: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub metadata: Value,
    pub remark: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateCustomer {
    pub name: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub metadata: Option<Value>,
    pub remark: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomerListQuery {
    pub keyword: Option<String>,
    pub status: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CustomerListMeta {
    pub page: u32,
    pub page_size: u32,
}

impl NewCustomer {
    pub fn new(
        tenant_id: Uuid,
        email: impl Into<String>,
        password_hash: Option<String>,
        name: Option<String>,
        phone: Option<String>,
        company: Option<String>,
        metadata: Value,
        remark: Option<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id,
            email: email.into(),
            password_hash,
            name,
            phone,
            company,
            metadata,
            remark,
        }
    }
}
