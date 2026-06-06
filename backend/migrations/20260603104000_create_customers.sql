create table customers (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  email text not null,
  password_hash text null,
  name text null,
  phone text null,
  company text null,
  status text not null default 'active',
  email_verified boolean not null default false,
  metadata jsonb not null default '{}'::jsonb,
  remark text null,
  last_login_at timestamptz null,
  last_login_ip inet null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint customers_status_check check (status in ('active', 'disabled', 'banned', 'pending'))
);

create unique index idx_customers_tenant_email
on customers(tenant_id, lower(email))
where deleted_at is null;

create index idx_customers_tenant_status
on customers(tenant_id, status);
