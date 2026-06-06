use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Tenant {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub status: String,
    pub plan: String,
    pub max_applications: i32,
    pub max_team_members: i32,
    pub max_customers: i32,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateTenantInput {
    pub name: String,
    pub slug: String,
    pub plan: Option<String>,
    pub max_applications: Option<i32>,
    pub max_team_members: Option<i32>,
    pub max_customers: Option<i32>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateTenantInput {
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct NewTenant {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub plan: String,
    pub max_applications: i32,
    pub max_team_members: i32,
    pub max_customers: i32,
    pub metadata: Value,
}

pub fn normalize_tenant_name(name: &str) -> Option<String> {
    let name = name.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

impl NewTenant {
    pub fn from_input(input: CreateTenantInput) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: input.name,
            slug: input.slug,
            plan: input.plan.unwrap_or_else(|| "free".to_owned()),
            max_applications: input.max_applications.unwrap_or(3),
            max_team_members: input.max_team_members.unwrap_or(5),
            max_customers: input.max_customers.unwrap_or(1000),
            metadata: input.metadata.unwrap_or_else(|| serde_json::json!({})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_tenant_name, CreateTenantInput, NewTenant};

    #[test]
    fn new_tenant_applies_defaults() {
        let tenant = NewTenant::from_input(CreateTenantInput {
            name: "Acme".to_owned(),
            slug: "acme".to_owned(),
            plan: None,
            max_applications: None,
            max_team_members: None,
            max_customers: None,
            metadata: None,
        });

        assert_eq!(tenant.name, "Acme");
        assert_eq!(tenant.slug, "acme");
        assert_eq!(tenant.plan, "free");
        assert_eq!(tenant.max_applications, 3);
        assert_eq!(tenant.max_team_members, 5);
        assert_eq!(tenant.max_customers, 1000);
        assert_eq!(tenant.metadata, serde_json::json!({}));
    }

    #[test]
    fn tenant_name_trims_and_rejects_blank() {
        assert_eq!(normalize_tenant_name(" Acme "), Some("Acme".to_owned()));
        assert_eq!(normalize_tenant_name(" "), None);
    }
}
