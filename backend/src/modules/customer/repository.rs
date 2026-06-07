use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::AppError,
    modules::customer::model::{Customer, CustomerListQuery, NewCustomer, UpdateCustomer},
};

#[derive(Clone)]
pub struct CustomerRepository {
    pool: PgPool,
}

impl CustomerRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn list(
        &self,
        tenant_id: Uuid,
        query: &CustomerListQuery,
    ) -> Result<Vec<Customer>, AppError> {
        let page = query.page.unwrap_or(1).max(1);
        let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
        let offset = ((page - 1) * page_size) as i64;
        let limit = page_size as i64;
        let keyword = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let keyword_pattern = keyword.map(|value| format!("%{}%", value.to_lowercase()));
        let status = query
            .status
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let include_history = query.include_history.unwrap_or(false);

        sqlx::query_as::<_, Customer>(
            r#"
            select
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              status,
              email_verified,
              metadata,
              remark,
              last_login_at,
              last_login_ip::text as last_login_ip,
              created_at,
              updated_at,
              deleted_at
            from customers
            where tenant_id = $1
              and deleted_at is null
              and ($2::text is null or status = $2)
              and ($2::text is not null or $4::bool or status not in ('disabled', 'banned'))
              and (
                $3::text is null
                or lower(email) like $3
                or lower(coalesce(name, '')) like $3
                or lower(coalesce(company, '')) like $3
              )
            order by created_at desc, id
            limit $5 offset $6
            "#,
        )
        .bind(tenant_id)
        .bind(status)
        .bind(keyword_pattern)
        .bind(include_history)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn create(&self, customer: NewCustomer) -> Result<Customer, AppError> {
        sqlx::query_as::<_, Customer>(
            r#"
            insert into customers (
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              metadata,
              remark
            )
            values ($1, $2, lower($3), $4, $5, $6, $7, $8, $9)
            returning
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              status,
              email_verified,
              metadata,
              remark,
              last_login_at,
              last_login_ip::text as last_login_ip,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(customer.id)
        .bind(customer.tenant_id)
        .bind(customer.email)
        .bind(customer.password_hash)
        .bind(customer.name)
        .bind(customer.phone)
        .bind(customer.company)
        .bind(customer.metadata)
        .bind(customer.remark)
        .fetch_one(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_id(
        &self,
        tenant_id: Uuid,
        id: Uuid,
    ) -> Result<Option<Customer>, AppError> {
        sqlx::query_as::<_, Customer>(
            r#"
            select
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              status,
              email_verified,
              metadata,
              remark,
              last_login_at,
              last_login_ip::text as last_login_ip,
              created_at,
              updated_at,
              deleted_at
            from customers
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn find_by_email(
        &self,
        tenant_id: Uuid,
        email: &str,
    ) -> Result<Option<Customer>, AppError> {
        sqlx::query_as::<_, Customer>(
            r#"
            select
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              status,
              email_verified,
              metadata,
              remark,
              last_login_at,
              last_login_ip::text as last_login_ip,
              created_at,
              updated_at,
              deleted_at
            from customers
            where tenant_id = $1
              and lower(email) = lower($2)
              and deleted_at is null
            "#,
        )
        .bind(tenant_id)
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn update(
        &self,
        tenant_id: Uuid,
        id: Uuid,
        input: UpdateCustomer,
    ) -> Result<Option<Customer>, AppError> {
        sqlx::query_as::<_, Customer>(
            r#"
            update customers
            set
              name = coalesce($3, name),
              phone = coalesce($4, phone),
              company = coalesce($5, company),
              metadata = coalesce($6, metadata),
              remark = coalesce($7, remark),
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
            returning
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              status,
              email_verified,
              metadata,
              remark,
              last_login_at,
              last_login_ip::text as last_login_ip,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .bind(input.name)
        .bind(input.phone)
        .bind(input.company)
        .bind(input.metadata)
        .bind(input.remark)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }

    pub async fn disable(&self, tenant_id: Uuid, id: Uuid) -> Result<Option<Customer>, AppError> {
        sqlx::query_as::<_, Customer>(
            r#"
            update customers
            set
              status = 'disabled',
              updated_at = now()
            where tenant_id = $1
              and id = $2
              and deleted_at is null
              and status <> 'disabled'
            returning
              id,
              tenant_id,
              email,
              password_hash,
              name,
              phone,
              company,
              status,
              email_verified,
              metadata,
              remark,
              last_login_at,
              last_login_ip::text as last_login_ip,
              created_at,
              updated_at,
              deleted_at
            "#,
        )
        .bind(tenant_id)
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(map_db_error)
    }
}

fn map_db_error(error: sqlx::Error) -> AppError {
    AppError::dependency(format!("customer repository database error: {error}"))
}
