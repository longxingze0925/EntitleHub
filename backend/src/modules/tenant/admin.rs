use axum::{extract::State, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
        tenant::{
            model::{normalize_tenant_name, Tenant, UpdateTenantInput},
            repository::TenantRepository,
        },
    },
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct TenantResponse {
    pub tenant: Tenant,
}

#[derive(Debug, Deserialize)]
pub struct DeleteTenantRequest {
    pub confirm: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteTenantResponse {
    pub deleted: bool,
    pub tenant_id: Uuid,
}

pub async fn get_tenant(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<TenantResponse>>, AppError> {
    ensure_admin_permission(&admin, "tenant:read")?;

    let tenant = TenantRepository::new(state.db.clone())
        .find_by_id(admin.tenant_id)
        .await?
        .ok_or_else(AppError::tenant_not_found)?;

    Ok(Json(ApiResponse::ok(
        TenantResponse { tenant },
        request_id.to_string(),
    )))
}

pub async fn update_tenant(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<UpdateTenantInput>,
) -> Result<Json<ApiResponse<TenantResponse>>, AppError> {
    ensure_admin_permission(&admin, "tenant:update")?;

    let name = normalize_tenant_name(&payload.name)
        .ok_or_else(|| AppError::validation_failed("tenant name is required"))?;
    let repository = TenantRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id)
        .await?
        .ok_or_else(AppError::tenant_not_found)?;
    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let tenant = update_tenant_name_in_transaction(&mut transaction, admin.tenant_id, &name)
        .await?
        .ok_or_else(AppError::tenant_not_found)?;
    audit_tenant_change(
        &mut transaction,
        &admin,
        &request_id,
        "tenant.update",
        Some(&before),
        Some(&tenant),
        json!({}),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        TenantResponse { tenant },
        request_id.to_string(),
    )))
}

async fn update_tenant_name_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
    name: &str,
) -> Result<Option<Tenant>, AppError> {
    sqlx::query_as::<_, Tenant>(
        r#"
        update tenants
        set
          name = $2,
          updated_at = now()
        where id = $1
          and deleted_at is null
        returning
          id,
          name,
          slug,
          status,
          plan,
          max_applications,
          max_team_members,
          max_customers,
          metadata,
          created_at,
          updated_at,
          deleted_at
        "#,
    )
    .bind(tenant_id)
    .bind(name)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)
}

pub async fn delete_tenant(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<DeleteTenantRequest>,
) -> Result<Json<ApiResponse<DeleteTenantResponse>>, AppError> {
    ensure_admin_permission(&admin, "tenant:delete")?;
    ensure_owner(&admin)?;
    ensure_delete_confirmed(&payload)?;

    let repository = TenantRepository::new(state.db.clone());
    let before = repository
        .find_by_id(admin.tenant_id)
        .await?
        .ok_or_else(AppError::tenant_not_found)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let deleted = soft_delete_tenant_in_transaction(&mut transaction, admin.tenant_id).await?;
    if !deleted {
        return Err(AppError::tenant_not_found());
    }

    let revoked_admin_refresh_tokens =
        revoke_tenant_admin_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id)
            .await?;
    let revoked_admin_sessions =
        revoke_tenant_admin_sessions_in_transaction(&mut transaction, admin.tenant_id).await?;
    let revoked_client_refresh_tokens =
        revoke_tenant_client_refresh_tokens_in_transaction(&mut transaction, admin.tenant_id)
            .await?;
    let revoked_client_sessions =
        revoke_tenant_client_sessions_in_transaction(&mut transaction, admin.tenant_id).await?;
    audit_tenant_change(
        &mut transaction,
        &admin,
        &request_id,
        "tenant.delete",
        Some(&before),
        None,
        json!({
            "confirm": "DELETE",
            "revoked_admin_sessions": revoked_admin_sessions,
            "revoked_admin_refresh_tokens": revoked_admin_refresh_tokens,
            "revoked_client_sessions": revoked_client_sessions,
            "revoked_client_refresh_tokens": revoked_client_refresh_tokens,
        }),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        DeleteTenantResponse {
            deleted,
            tenant_id: admin.tenant_id,
        },
        request_id.to_string(),
    )))
}

async fn audit_tenant_change(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: Option<&Tenant>,
    after: Option<&Tenant>,
    metadata: Value,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "tenant",
            resource_id: Some(admin.tenant_id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: before.map(tenant_audit_json),
            after_json: after.map(tenant_audit_json),
            metadata_json: metadata,
        },
    )
    .await
}

async fn soft_delete_tenant_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
) -> Result<bool, AppError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        update tenants
        set
          status = 'deleted',
          deleted_at = now(),
          updated_at = now()
        where id = $1
          and deleted_at is null
        returning id
        "#,
    )
    .bind(tenant_id)
    .fetch_optional(&mut **transaction)
    .await
    .map(|tenant_id| tenant_id.is_some())
    .map_err(map_db_error)
}

async fn revoke_tenant_admin_sessions_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_sessions
        set revoked_at = now()
        where tenant_id = $1
          and revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_tenant_admin_refresh_tokens_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update admin_refresh_tokens rt
        set revoked_at = now()
        from admin_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and rt.revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_tenant_client_sessions_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_sessions
        set revoked_at = now()
        where tenant_id = $1
          and revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

async fn revoke_tenant_client_refresh_tokens_in_transaction(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant_id: Uuid,
) -> Result<u64, AppError> {
    sqlx::query(
        r#"
        update client_refresh_tokens rt
        set revoked_at = now()
        from client_sessions s
        where rt.session_id = s.id
          and s.tenant_id = $1
          and rt.revoked_at is null
        "#,
    )
    .bind(tenant_id)
    .execute(&mut **transaction)
    .await
    .map(|result| result.rows_affected())
    .map_err(map_db_error)
}

fn tenant_audit_json(tenant: &Tenant) -> Value {
    json!({
        "id": tenant.id,
        "name": tenant.name,
        "slug": tenant.slug,
        "status": tenant.status,
        "plan": tenant.plan,
    })
}

fn ensure_admin_permission(admin: &AdminContext, permission_code: &str) -> Result<(), AppError> {
    if admin
        .permissions
        .iter()
        .any(|permission| permission == permission_code)
    {
        return Ok(());
    }

    Err(AppError::forbidden(format!(
        "missing permission: {permission_code}"
    )))
}

fn ensure_owner(admin: &AdminContext) -> Result<(), AppError> {
    if admin.roles.iter().any(|role| role == "owner") {
        return Ok(());
    }

    Err(AppError::forbidden("tenant delete requires owner role"))
}

fn ensure_delete_confirmed(payload: &DeleteTenantRequest) -> Result<(), AppError> {
    if payload.confirm.trim() == "DELETE" {
        return Ok(());
    }

    Err(AppError::validation_failed("confirm must be DELETE"))
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("tenant admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::modules::auth::session::AdminContext;

    use super::{ensure_delete_confirmed, ensure_owner, DeleteTenantRequest};

    #[test]
    fn delete_confirmation_requires_delete_word() {
        assert!(ensure_delete_confirmed(&DeleteTenantRequest {
            confirm: "DELETE".to_owned(),
        })
        .is_ok());
        assert!(ensure_delete_confirmed(&DeleteTenantRequest {
            confirm: "delete".to_owned(),
        })
        .is_err());
    }

    #[test]
    fn tenant_delete_requires_owner_role() {
        let mut admin = AdminContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            team_member_id: Uuid::nil(),
            email: "owner@example.com".to_owned(),
            name: "Owner".to_owned(),
            email_verified: true,
            mfa_enabled: false,
            tenant_name: "Default".to_owned(),
            roles: vec!["admin".to_owned()],
            permissions: vec!["tenant:delete".to_owned()],
        };

        assert!(ensure_owner(&admin).is_err());
        admin.roles.push("owner".to_owned());
        assert!(ensure_owner(&admin).is_ok());
    }
}
