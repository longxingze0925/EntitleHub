create table devices (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  customer_id uuid null references customers(id),
  license_id uuid null references licenses(id),
  subscription_id uuid null,
  machine_id text not null,
  device_name text null,
  os text null,
  app_version text null,
  status text not null default 'active',
  first_seen_at timestamptz not null default now(),
  last_seen_at timestamptz null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint devices_status_check check (status in ('active', 'disabled', 'blacklisted', 'unbound')),
  constraint devices_machine_id_not_blank check (length(btrim(machine_id)) > 0)
);

create unique index idx_devices_app_machine
on devices(tenant_id, app_id, machine_id)
where deleted_at is null;

create index idx_devices_license on devices(license_id);
create index idx_devices_subscription on devices(subscription_id);
create index idx_devices_status on devices(tenant_id, app_id, status);

create table device_blacklist (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid null references applications(id),
  machine_id text not null,
  reason text null,
  created_by uuid null references team_members(id),
  created_at timestamptz not null default now(),

  constraint device_blacklist_machine_id_not_blank check (length(btrim(machine_id)) > 0)
);

create unique index idx_device_blacklist_scope
on device_blacklist(
  tenant_id,
  coalesce(app_id, '00000000-0000-0000-0000-000000000000'::uuid),
  machine_id
);
