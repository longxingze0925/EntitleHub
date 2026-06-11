use std::env;

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    crypto::{password::hash_password, token::generate_token},
    error::AppError,
};

#[derive(Debug, Clone)]
pub struct BootstrapOwnerInput {
    pub tenant_name: String,
    pub tenant_slug: String,
    pub owner_email: String,
    pub owner_name: String,
    pub owner_password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BootstrapOwnerResult {
    pub tenant_id: Uuid,
    pub owner_id: Uuid,
    pub generated_password: Option<String>,
}

impl BootstrapOwnerInput {
    pub fn from_env() -> Result<Self, AppError> {
        Ok(Self {
            tenant_name: required_env("INIT_TENANT_NAME")?,
            tenant_slug: required_env("INIT_TENANT_SLUG")?,
            owner_email: required_env("INIT_OWNER_EMAIL")?,
            owner_name: required_env("INIT_OWNER_NAME")?,
            owner_password: env::var("INIT_OWNER_PASSWORD").ok(),
        })
    }

    fn resolve_password(&self) -> ResolvedPassword {
        match &self.owner_password {
            Some(password) => ResolvedPassword {
                password: password.clone(),
                generated: false,
            },
            None => ResolvedPassword {
                password: generate_token(),
                generated: true,
            },
        }
    }
}

struct ResolvedPassword {
    password: String,
    generated: bool,
}

pub async fn initialize_owner(
    pool: &PgPool,
    input: BootstrapOwnerInput,
) -> Result<BootstrapOwnerResult, AppError> {
    let mut transaction = pool.begin().await.map_err(map_db_error)?;

    let existing_tenants = sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from tenants
        where deleted_at is null
        "#,
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    if existing_tenants > 0 {
        return Err(AppError::config(
            "init-owner can only run when no active tenants exist",
        ));
    }

    let resolved_password = input.resolve_password();
    let password_hash = hash_password(&resolved_password.password)?;
    let tenant_id = Uuid::new_v4();
    let owner_id = Uuid::new_v4();
    let owner_role_id = Uuid::new_v4();
    let admin_role_id = Uuid::new_v4();
    let developer_role_id = Uuid::new_v4();
    let viewer_role_id = Uuid::new_v4();

    sqlx::query(
        r#"
        insert into tenants (
          id,
          name,
          slug
        )
        values ($1, $2, $3)
        "#,
    )
    .bind(tenant_id)
    .bind(input.tenant_name)
    .bind(input.tenant_slug)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    sqlx::query(
        r#"
        insert into team_members (
          id,
          tenant_id,
          email,
          password_hash,
          name,
          status
        )
        values ($1, $2, lower($3), $4, $5, 'active')
        "#,
    )
    .bind(owner_id)
    .bind(tenant_id)
    .bind(input.owner_email)
    .bind(password_hash)
    .bind(input.owner_name)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    insert_builtin_role(
        &mut transaction,
        owner_role_id,
        tenant_id,
        "owner",
        "所有者",
        "拥有租户全部权限",
    )
    .await?;
    insert_builtin_role(
        &mut transaction,
        admin_role_id,
        tenant_id,
        "admin",
        "管理员",
        "拥有大部分管理权限",
    )
    .await?;
    insert_builtin_role(
        &mut transaction,
        developer_role_id,
        tenant_id,
        "developer",
        "开发者",
        "负责应用、版本、脚本和设备相关操作",
    )
    .await?;
    insert_builtin_role(
        &mut transaction,
        viewer_role_id,
        tenant_id,
        "viewer",
        "查看者",
        "只读查看租户数据",
    )
    .await?;

    sqlx::query(
        r#"
        insert into role_permissions (role_id, permission_id)
        select $1, id
        from permissions
        "#,
    )
    .bind(owner_role_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    attach_permission_set(&mut transaction, admin_role_id, ADMIN_PERMISSIONS).await?;
    attach_permission_set(&mut transaction, developer_role_id, DEVELOPER_PERMISSIONS).await?;
    attach_permission_set(&mut transaction, viewer_role_id, VIEWER_PERMISSIONS).await?;

    sqlx::query(
        r#"
        insert into team_member_roles (
          team_member_id,
          role_id
        )
        values ($1, $2)
        "#,
    )
    .bind(owner_id)
    .bind(owner_role_id)
    .execute(&mut *transaction)
    .await
    .map_err(map_db_error)?;

    transaction.commit().await.map_err(map_db_error)?;

    Ok(BootstrapOwnerResult {
        tenant_id,
        owner_id,
        generated_password: resolved_password
            .generated
            .then_some(resolved_password.password),
    })
}

async fn insert_builtin_role(
    transaction: &mut Transaction<'_, Postgres>,
    id: Uuid,
    tenant_id: Uuid,
    code: &'static str,
    name: &'static str,
    description: &'static str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        insert into roles (
          id,
          tenant_id,
          code,
          name,
          description,
          builtin
        )
        values ($1, $2, $3, $4, $5, true)
        "#,
    )
    .bind(id)
    .bind(tenant_id)
    .bind(code)
    .bind(name)
    .bind(description)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn attach_permission_set(
    transaction: &mut Transaction<'_, Postgres>,
    role_id: Uuid,
    permission_codes: &[&'static str],
) -> Result<(), AppError> {
    for permission_code in permission_codes {
        let result = sqlx::query(
            r#"
            insert into role_permissions (role_id, permission_id)
            select $1, id
            from permissions
            where code = $2
            "#,
        )
        .bind(role_id)
        .bind(permission_code)
        .execute(&mut **transaction)
        .await
        .map_err(map_db_error)?;
        if result.rows_affected() == 0 {
            return Err(AppError::config(format!(
                "bootstrap permission code does not exist: {permission_code}"
            )));
        }
    }

    Ok(())
}

fn required_env(key: &str) -> Result<String, AppError> {
    env::var(key).map_err(|_| AppError::config(format!("{key} is required")))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("bootstrap owner database error: {error}"))
}

const ADMIN_PERMISSIONS: &[&str] = &[
    "tenant:read",
    "tenant:update",
    "member:read",
    "member:invite",
    "member:update",
    "member:disable",
    "role:read",
    "role:create",
    "role:update",
    "role:delete",
    "permission:read",
    "customer:read",
    "customer:create",
    "customer:update",
    "customer:disable",
    "app:read",
    "app:create",
    "app:update",
    "app:rotate_key",
    "app:read_key",
    "license:read",
    "license:create",
    "license:revoke",
    "license:suspend",
    "license:renew",
    "license:reset_device",
    "subscription:read",
    "subscription:create",
    "subscription:update",
    "subscription:cancel",
    "subscription:renew",
    "subscription:suspend",
    "subscription:resume",
    "subscription:reset_device",
    "device:read",
    "device:unbind",
    "device:blacklist",
    "device:unblacklist",
    "release:read",
    "release:upload",
    "release:create",
    "release:update",
    "release:publish",
    "release:deprecate",
    "release:delete",
    "script:read",
    "script:create",
    "script:update",
    "script:publish",
    "script:deprecate",
    "script:revoke",
    "audit:read",
    "audit:export",
    "system:read",
    "system:update",
    "ai:read",
    "ai:job:read",
    "ai:job:update",
    "ai:api_key:update",
    "ai:asset:delete",
    "ai:provider:update",
    "ai:model:update",
    "ai:wallet:update",
    "server_api_key:read",
    "server_api_key:update",
    "notification:read",
    "notification:update",
    "security:view_events",
    "security:retry_event",
];

const DEVELOPER_PERMISSIONS: &[&str] = &[
    "app:read",
    "release:read",
    "release:upload",
    "release:create",
    "script:read",
    "script:create",
    "device:read",
    "ai:read",
    "ai:job:read",
    "ai:job:update",
];

const VIEWER_PERMISSIONS: &[&str] = &[
    "tenant:read",
    "member:read",
    "customer:read",
    "app:read",
    "license:read",
    "device:read",
    "release:read",
    "script:read",
    "audit:read",
    "ai:read",
    "ai:job:read",
    "notification:read",
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        BootstrapOwnerInput, ADMIN_PERMISSIONS, DEVELOPER_PERMISSIONS, VIEWER_PERMISSIONS,
    };

    #[test]
    fn resolve_password_uses_env_password_when_present() {
        let input = BootstrapOwnerInput {
            tenant_name: "Acme".to_owned(),
            tenant_slug: "acme".to_owned(),
            owner_email: "owner@example.com".to_owned(),
            owner_name: "Owner".to_owned(),
            owner_password: Some("Password@123456".to_owned()),
        };

        let password = input.resolve_password();

        assert_eq!(password.password, "Password@123456");
        assert!(!password.generated);
    }

    #[test]
    fn resolve_password_generates_when_missing() {
        let input = BootstrapOwnerInput {
            tenant_name: "Acme".to_owned(),
            tenant_slug: "acme".to_owned(),
            owner_email: "owner@example.com".to_owned(),
            owner_name: "Owner".to_owned(),
            owner_password: None,
        };

        let password = input.resolve_password();

        assert!(password.generated);
        assert!(password.password.len() >= 40);
    }

    #[test]
    fn builtin_role_permissions_exist_in_seed_migration() {
        let permission_migrations = permission_migration_text();

        assert_permission_set_exists(&permission_migrations, "admin", ADMIN_PERMISSIONS);
        assert_permission_set_exists(&permission_migrations, "developer", DEVELOPER_PERMISSIONS);
        assert_permission_set_exists(&permission_migrations, "viewer", VIEWER_PERMISSIONS);
    }

    #[test]
    fn handler_permission_strings_exist_in_migrations() {
        let permission_migrations = permission_migration_text();

        for source in HANDLER_PERMISSION_SOURCES {
            for permission_code in permission_like_strings(source) {
                assert!(
                    permission_migrations.contains(&format!("'{permission_code}'")),
                    "missing handler permission in permission migrations: {permission_code}"
                );
            }
        }
    }

    const HANDLER_PERMISSION_SOURCES: &[&str] = &[
        include_str!("ai/admin.rs"),
        include_str!("ai/api_keys.rs"),
        include_str!("ai/jobs.rs"),
        include_str!("ai/usage.rs"),
        include_str!("application/admin.rs"),
        include_str!("audit/admin.rs"),
        include_str!("customer/admin.rs"),
        include_str!("device/admin.rs"),
        include_str!("iam/admin.rs"),
        include_str!("license/admin.rs"),
        include_str!("outbox/admin.rs"),
        include_str!("notification/admin.rs"),
        include_str!("release/admin.rs"),
        include_str!("secure_script/admin.rs"),
        include_str!("server_api.rs"),
        include_str!("subscription/admin.rs"),
        include_str!("system/admin.rs"),
        include_str!("team/admin.rs"),
        include_str!("tenant/admin.rs"),
    ];

    fn permission_migration_text() -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            include_str!("../../migrations/20260603101000_create_rbac_tables.sql"),
            include_str!("../../migrations/20260604102000_add_security_retry_event_permission.sql"),
            include_str!("../../migrations/20260604105000_add_script_deprecate_permission.sql"),
            include_str!("../../migrations/20260606150000_create_notification_channels.sql"),
            include_str!("../../migrations/20260608090000_create_ai_billing_tables.sql"),
            include_str!("../../migrations/20260608093000_create_ai_gateway_api_keys.sql"),
            include_str!("../../migrations/20260608103000_add_ai_gateway_controls.sql"),
            include_str!("../../migrations/20260609102000_add_subscription_admin_actions.sql"),
            include_str!("../../migrations/20260609113000_grant_release_update_delete.sql"),
            include_str!("../../migrations/20260610100000_create_server_api_keys.sql"),
            include_str!("../../migrations/20260611120000_create_ai_generation_jobs.sql"),
            include_str!("../../migrations/20260611143000_add_ai_generation_job_actions.sql")
        )
    }

    fn assert_permission_set_exists(
        permission_migrations: &str,
        role: &str,
        permission_codes: &[&'static str],
    ) {
        let mut seen = HashSet::new();

        for permission_code in permission_codes {
            assert!(
                seen.insert(*permission_code),
                "duplicate {role} permission code: {permission_code}"
            );
            assert!(
                permission_migrations.contains(&format!("'{permission_code}'")),
                "missing {role} permission in permission migrations: {permission_code}"
            );
        }
    }

    fn permission_like_strings(source: &'static str) -> Vec<&'static str> {
        let mut values = Vec::new();
        let mut rest = source;

        while let Some(start) = rest.find('"') {
            let after_start = &rest[start + 1..];
            let Some(end) = after_start.find('"') else {
                break;
            };
            let value = &after_start[..end];
            if is_permission_code(value) {
                values.push(value);
            }
            rest = &after_start[end + 1..];
        }

        values
    }

    fn is_permission_code(value: &str) -> bool {
        let Some((resource, action)) = value.split_once(':') else {
            return false;
        };
        !resource.is_empty()
            && !action.is_empty()
            && resource
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '_')
            && action
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch == '_')
    }
}
