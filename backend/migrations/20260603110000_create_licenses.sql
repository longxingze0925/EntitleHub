create table licenses (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  customer_id uuid null references customers(id),
  license_key_hash text not null,
  type text not null default 'standard',
  status text not null default 'active',
  max_devices int not null default 1,
  features jsonb not null default '[]'::jsonb,
  starts_at timestamptz null,
  expires_at timestamptz null,
  revoked_at timestamptz null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint licenses_status_check check (status in ('active', 'suspended', 'revoked', 'expired')),
  constraint licenses_type_check check (type in ('standard', 'trial', 'enterprise')),
  constraint licenses_max_devices_check check (max_devices >= 0),
  constraint licenses_validity_window_check check (
    starts_at is null or expires_at is null or expires_at > starts_at
  )
);

create unique index idx_licenses_key_hash on licenses(license_key_hash);
create index idx_licenses_tenant_app on licenses(tenant_id, app_id);
create index idx_licenses_customer on licenses(customer_id);
create index idx_licenses_status_expires on licenses(status, expires_at);
