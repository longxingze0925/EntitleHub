use axum::{
    extract::{Path, State},
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    error::{ApiResponse, AppError},
    http::request_id::RequestId,
    modules::{
        audit::{self, AuditLogInput},
        auth::session::AdminContext,
    },
    state::AppState,
};

#[derive(Debug, Clone, Serialize)]
pub struct RoleDetail {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub builtin: bool,
    pub permission_codes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PermissionSummary {
    pub code: String,
    pub name: String,
    pub resource: String,
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct RoleListResponse {
    pub items: Vec<RoleDetail>,
}

#[derive(Debug, Serialize)]
pub struct RoleMutationResponse {
    pub role: RoleDetail,
}

#[derive(Debug, Serialize)]
pub struct RoleDeleteResponse {
    pub deleted: bool,
    pub role_id: Uuid,
}

#[derive(Debug, Serialize)]
pub struct PermissionListResponse {
    pub items: Vec<PermissionSummary>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRoleRequest {
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub permission_codes: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub name: String,
    pub description: Option<String>,
    pub permission_codes: Vec<String>,
}

#[derive(Debug, Clone, FromRow)]
struct RoleRow {
    id: Uuid,
    code: String,
    name: String,
    description: Option<String>,
    builtin: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

pub async fn list_roles(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<RoleListResponse>>, AppError> {
    ensure_admin_permission(&admin, "role:read")?;

    let rows = list_role_rows(&state, admin.tenant_id).await?;
    let roles = hydrate_roles(&state, admin.tenant_id, rows).await?;

    Ok(Json(ApiResponse::ok(
        RoleListResponse { items: roles },
        request_id.to_string(),
    )))
}

pub async fn list_permissions(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
) -> Result<Json<ApiResponse<PermissionListResponse>>, AppError> {
    ensure_admin_permission(&admin, "permission:read")?;

    let permissions = sqlx::query_as::<_, PermissionSummary>(
        r#"
        select code, name, resource, action
        from permissions
        order by resource asc, action asc, code asc
        "#,
    )
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        PermissionListResponse { items: permissions },
        request_id.to_string(),
    )))
}

pub async fn create_role(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Json(payload): Json<CreateRoleRequest>,
) -> Result<Json<ApiResponse<RoleMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "role:create")?;
    let code = normalize_role_code(&payload.code)?;
    let name = normalize_role_name(&payload.name)?;
    let description = clean_optional(payload.description);
    let permission_codes = normalize_permission_codes(payload.permission_codes)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    ensure_role_code_available(&mut transaction, admin.tenant_id, &code).await?;
    let role = insert_role(&mut transaction, admin.tenant_id, &code, &name, description).await?;
    replace_role_permissions(
        &mut transaction,
        admin.tenant_id,
        role.id,
        &permission_codes,
    )
    .await?;
    let detail = hydrate_role_in_transaction(&mut transaction, admin.tenant_id, role).await?;
    audit_role_change(
        &mut transaction,
        &admin,
        &request_id,
        "role.create",
        None,
        Some(&detail),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RoleMutationResponse { role: detail },
        request_id.to_string(),
    )))
}

pub async fn update_role(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(role_id): Path<Uuid>,
    Json(payload): Json<UpdateRoleRequest>,
) -> Result<Json<ApiResponse<RoleMutationResponse>>, AppError> {
    ensure_admin_permission(&admin, "role:update")?;
    let name = normalize_role_name(&payload.name)?;
    let description = clean_optional(payload.description);
    let permission_codes = normalize_permission_codes(payload.permission_codes)?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before_row = find_role_for_update(&mut transaction, admin.tenant_id, role_id).await?;
    ensure_custom_role(&before_row)?;
    let before =
        hydrate_role_in_transaction(&mut transaction, admin.tenant_id, before_row.clone()).await?;
    let role = update_role_row(
        &mut transaction,
        admin.tenant_id,
        role_id,
        &name,
        description,
    )
    .await?;
    replace_role_permissions(
        &mut transaction,
        admin.tenant_id,
        role.id,
        &permission_codes,
    )
    .await?;
    let detail = hydrate_role_in_transaction(&mut transaction, admin.tenant_id, role).await?;
    audit_role_change(
        &mut transaction,
        &admin,
        &request_id,
        "role.update",
        Some(&before),
        Some(&detail),
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RoleMutationResponse { role: detail },
        request_id.to_string(),
    )))
}

pub async fn delete_role(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminContext>,
    Extension(request_id): Extension<RequestId>,
    Path(role_id): Path<Uuid>,
) -> Result<Json<ApiResponse<RoleDeleteResponse>>, AppError> {
    ensure_admin_permission(&admin, "role:delete")?;

    let mut transaction = state.db.begin().await.map_err(map_db_error)?;
    let before_row = find_role_for_update(&mut transaction, admin.tenant_id, role_id).await?;
    ensure_custom_role(&before_row)?;
    ensure_role_unassigned(&mut transaction, admin.tenant_id, role_id).await?;
    let before = hydrate_role_in_transaction(&mut transaction, admin.tenant_id, before_row).await?;
    soft_delete_role(&mut transaction, admin.tenant_id, role_id).await?;
    audit_role_change(
        &mut transaction,
        &admin,
        &request_id,
        "role.delete",
        Some(&before),
        None,
    )
    .await?;
    transaction.commit().await.map_err(map_db_error)?;

    Ok(Json(ApiResponse::ok(
        RoleDeleteResponse {
            deleted: true,
            role_id,
        },
        request_id.to_string(),
    )))
}

async fn list_role_rows(state: &AppState, tenant_id: Uuid) -> Result<Vec<RoleRow>, AppError> {
    sqlx::query_as::<_, RoleRow>(&role_select_sql(
        "where tenant_id = $1 and deleted_at is null order by builtin desc, code asc",
    ))
    .bind(tenant_id)
    .fetch_all(&state.db)
    .await
    .map_err(map_db_error)
}

async fn hydrate_roles(
    state: &AppState,
    tenant_id: Uuid,
    rows: Vec<RoleRow>,
) -> Result<Vec<RoleDetail>, AppError> {
    let mut roles = Vec::with_capacity(rows.len());
    for row in rows {
        let permission_codes = list_permission_codes_for_role(&state.db, tenant_id, row.id).await?;
        roles.push(RoleDetail::from_row(row, permission_codes));
    }

    Ok(roles)
}

async fn hydrate_role_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    row: RoleRow,
) -> Result<RoleDetail, AppError> {
    let permission_codes =
        list_permission_codes_for_role_in_transaction(transaction, tenant_id, row.id).await?;

    Ok(RoleDetail::from_row(row, permission_codes))
}

async fn find_role_for_update(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<RoleRow, AppError> {
    sqlx::query_as::<_, RoleRow>(&format!(
        "{} for update",
        role_select_sql("where tenant_id = $1 and id = $2 and deleted_at is null")
    ))
    .bind(tenant_id)
    .bind(role_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(map_db_error)?
    .ok_or_else(|| AppError::not_found("role not found"))
}

async fn ensure_role_code_available(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    code: &str,
) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        select exists (
          select 1
          from roles
          where tenant_id = $1
            and code = $2
            and deleted_at is null
        )
        "#,
    )
    .bind(tenant_id)
    .bind(code)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)?;
    if exists {
        return Err(AppError::conflict("role code already exists"));
    }

    Ok(())
}

async fn insert_role(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    code: &str,
    name: &str,
    description: Option<String>,
) -> Result<RoleRow, AppError> {
    sqlx::query_as::<_, RoleRow>(
        r#"
        insert into roles (
          id,
          tenant_id,
          code,
          name,
          description,
          builtin
        )
        values ($1, $2, $3, $4, $5, false)
        returning
          id,
          code,
          name,
          description,
          builtin,
          created_at,
          updated_at
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant_id)
    .bind(code)
    .bind(name)
    .bind(description)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn update_role_row(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    role_id: Uuid,
    name: &str,
    description: Option<String>,
) -> Result<RoleRow, AppError> {
    sqlx::query_as::<_, RoleRow>(
        r#"
        update roles
        set
          name = $3,
          description = $4,
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        returning
          id,
          code,
          name,
          description,
          builtin,
          created_at,
          updated_at
        "#,
    )
    .bind(tenant_id)
    .bind(role_id)
    .bind(name)
    .bind(description)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn replace_role_permissions(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    role_id: Uuid,
    permission_codes: &[String],
) -> Result<(), AppError> {
    let permission_ids = resolve_permission_ids(transaction, permission_codes).await?;
    sqlx::query(
        r#"
        delete from role_permissions
        where role_id in (
          select id
          from roles
          where tenant_id = $1
            and id = $2
            and deleted_at is null
        )
        "#,
    )
    .bind(tenant_id)
    .bind(role_id)
    .execute(&mut **transaction)
    .await
    .map_err(map_db_error)?;

    for permission_id in permission_ids {
        sqlx::query(
            r#"
            insert into role_permissions (
              role_id,
              permission_id
            )
            select r.id, $3
            from roles r
            where r.tenant_id = $1
              and r.id = $2
              and r.deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(role_id)
        .bind(permission_id)
        .execute(&mut **transaction)
        .await
        .map_err(map_db_error)?;
    }

    Ok(())
}

async fn resolve_permission_ids(
    transaction: &mut Transaction<'_, Postgres>,
    permission_codes: &[String],
) -> Result<Vec<Uuid>, AppError> {
    let rows = sqlx::query_as::<_, PermissionIdRow>(
        r#"
        select id, code
        from permissions
        where code = any($1)
        order by code
        "#,
    )
    .bind(permission_codes)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_db_error)?;
    if rows.len() != permission_codes.len() {
        return Err(AppError::validation_failed(
            "permission_codes contain unknown code",
        ));
    }

    Ok(rows.into_iter().map(|row| row.id).collect())
}

#[derive(Debug, FromRow)]
struct PermissionIdRow {
    id: Uuid,
    #[allow(dead_code)]
    code: String,
}

async fn list_permission_codes_for_role(
    pool: &sqlx::PgPool,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<Vec<String>, AppError> {
    sqlx::query_scalar::<_, String>(
        r#"
        select p.code
        from roles r
        join role_permissions rp
          on rp.role_id = r.id
        join permissions p
          on p.id = rp.permission_id
        where r.tenant_id = $1
          and r.id = $2
          and r.deleted_at is null
        order by p.code
        "#,
    )
    .bind(tenant_id)
    .bind(role_id)
    .fetch_all(pool)
    .await
    .map_err(map_db_error)
}

async fn list_permission_codes_for_role_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<Vec<String>, AppError> {
    sqlx::query_scalar::<_, String>(
        r#"
        select p.code
        from roles r
        join role_permissions rp
          on rp.role_id = r.id
        join permissions p
          on p.id = rp.permission_id
        where r.tenant_id = $1
          and r.id = $2
          and r.deleted_at is null
        order by p.code
        "#,
    )
    .bind(tenant_id)
    .bind(role_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(map_db_error)
}

async fn ensure_role_unassigned(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<(), AppError> {
    let assigned = sqlx::query_scalar::<_, i64>(
        r#"
        select count(*)
        from team_member_roles tmr
        join roles r
          on r.id = tmr.role_id
         and r.tenant_id = $1
        join team_members tm
          on tm.id = tmr.team_member_id
         and tm.tenant_id = r.tenant_id
        where tmr.role_id = $2
        "#,
    )
    .bind(tenant_id)
    .bind(role_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(map_db_error)?;
    if assigned > 0 {
        return Err(AppError::conflict("assigned role cannot be deleted"));
    }

    Ok(())
}

async fn soft_delete_role(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    role_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        update roles
        set
          deleted_at = now(),
          updated_at = now()
        where tenant_id = $1
          and id = $2
          and deleted_at is null
        "#,
    )
    .bind(tenant_id)
    .bind(role_id)
    .execute(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(map_db_error)
}

async fn audit_role_change(
    transaction: &mut Transaction<'_, Postgres>,
    admin: &AdminContext,
    request_id: &RequestId,
    action: &'static str,
    before: Option<&RoleDetail>,
    after: Option<&RoleDetail>,
) -> Result<(), AppError> {
    audit::record(
        transaction,
        AuditLogInput {
            tenant_id: Some(admin.tenant_id),
            actor_type: "team_member",
            actor_id: Some(admin.team_member_id),
            action,
            resource_type: "role",
            resource_id: before.or(after).map(|role| role.id),
            ip: None,
            user_agent: None,
            request_id: Some(request_id.to_string()),
            before_json: before.map(role_audit_json),
            after_json: after.map(role_audit_json),
            metadata_json: json!({}),
        },
    )
    .await
}

impl RoleDetail {
    fn from_row(row: RoleRow, permission_codes: Vec<String>) -> Self {
        Self {
            id: row.id,
            code: row.code,
            name: row.name,
            description: row.description,
            builtin: row.builtin,
            permission_codes,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

fn role_select_sql(where_clause: &str) -> String {
    format!(
        r#"
        select
          id,
          code,
          name,
          description,
          builtin,
          created_at,
          updated_at
        from roles
        {where_clause}
        "#
    )
}

fn role_audit_json(role: &RoleDetail) -> Value {
    json!({
        "id": role.id,
        "code": &role.code,
        "name": &role.name,
        "description": &role.description,
        "builtin": role.builtin,
        "permission_codes": &role.permission_codes,
    })
}

fn normalize_role_code(code: &str) -> Result<String, AppError> {
    let code = code.trim().to_ascii_lowercase();
    if code.is_empty() || code.len() > 64 {
        return Err(AppError::validation_failed("role code is invalid"));
    }
    if !code
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(AppError::validation_failed("role code is invalid"));
    }

    Ok(code)
}

fn normalize_role_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err(AppError::validation_failed("role name is invalid"));
    }

    Ok(name.to_owned())
}

fn normalize_permission_codes(permission_codes: Vec<String>) -> Result<Vec<String>, AppError> {
    let mut codes = permission_codes
        .into_iter()
        .map(|code| code.trim().to_owned())
        .filter(|code| !code.is_empty())
        .collect::<Vec<_>>();
    codes.sort();
    codes.dedup();
    for code in &codes {
        if !code
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
        {
            return Err(AppError::validation_failed("permission code is invalid"));
        }
    }

    Ok(codes)
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(value)
    })
}

fn ensure_custom_role(role: &RoleRow) -> Result<(), AppError> {
    if !role.builtin {
        return Ok(());
    }

    Err(AppError::conflict("builtin role cannot be changed"))
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

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("iam admin database error: {error}"))
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use crate::modules::auth::session::AdminContext;

    use super::{
        clean_optional, ensure_admin_permission, ensure_custom_role, normalize_permission_codes,
        normalize_role_code, normalize_role_name, RoleRow,
    };

    #[test]
    fn permission_check_requires_role_read() {
        let admin = AdminContext {
            session_id: Uuid::nil(),
            tenant_id: Uuid::nil(),
            team_member_id: Uuid::nil(),
            email: "admin@example.com".to_owned(),
            name: "Admin".to_owned(),
            email_verified: true,
            mfa_enabled: false,
            tenant_name: "Default".to_owned(),
            roles: vec!["admin".to_owned()],
            permissions: vec!["role:read".to_owned()],
        };

        assert!(ensure_admin_permission(&admin, "role:read").is_ok());
        assert!(ensure_admin_permission(&admin, "role:update").is_err());
    }

    #[test]
    fn role_code_normalizes_and_rejects_invalid_values() {
        assert_eq!(
            normalize_role_code(" Support_Tier-1 ").expect("code"),
            "support_tier-1"
        );
        assert!(normalize_role_code("bad code").is_err());
    }

    #[test]
    fn role_name_trims_and_rejects_blank() {
        assert_eq!(normalize_role_name(" Support ").expect("name"), "Support");
        assert!(normalize_role_name(" ").is_err());
    }

    #[test]
    fn permission_codes_are_sorted_deduped_and_validated() {
        assert_eq!(
            normalize_permission_codes(vec![
                "app:read".to_owned(),
                " tenant:update ".to_owned(),
                "app:read".to_owned(),
            ])
            .expect("permission codes"),
            vec!["app:read".to_owned(), "tenant:update".to_owned()]
        );
        assert!(normalize_permission_codes(vec!["bad code".to_owned()]).is_err());
    }

    #[test]
    fn builtin_roles_are_not_mutable() {
        let mut role = test_role_row(false);
        assert!(ensure_custom_role(&role).is_ok());

        role.builtin = true;
        assert!(ensure_custom_role(&role).is_err());
    }

    #[test]
    fn clean_optional_trims_blank() {
        assert_eq!(
            clean_optional(Some(" role ".to_owned())),
            Some("role".to_owned())
        );
        assert_eq!(clean_optional(Some(" ".to_owned())), None);
    }

    fn test_role_row(builtin: bool) -> RoleRow {
        RoleRow {
            id: Uuid::nil(),
            code: "support".to_owned(),
            name: "Support".to_owned(),
            description: None,
            builtin,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
