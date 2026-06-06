use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct TeamMember {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub phone: Option<String>,
    pub avatar: Option<String>,
    pub status: String,
    pub email_verified: bool,
    pub mfa_enabled: bool,
    pub mfa_secret_encrypted: Option<String>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub last_login_ip: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTeamMemberInput {
    pub tenant_id: Uuid,
    pub email: String,
    pub password: String,
    pub name: String,
    pub phone: Option<String>,
    pub avatar: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTeamMember {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub phone: Option<String>,
    pub avatar: Option<String>,
}

impl NewTeamMember {
    pub fn from_input(input: CreateTeamMemberInput, password_hash: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: input.tenant_id,
            email: input.email,
            password_hash,
            name: input.name,
            phone: input.phone,
            avatar: input.avatar,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewInvitedTeamMember {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
}

impl NewInvitedTeamMember {
    pub fn new(
        tenant_id: Uuid,
        email: impl Into<String>,
        password_hash: impl Into<String>,
    ) -> Self {
        let email = email.into();

        Self {
            id: Uuid::new_v4(),
            tenant_id,
            password_hash: password_hash.into(),
            name: email.clone(),
            email,
        }
    }
}
