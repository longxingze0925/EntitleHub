create table device_keys (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  device_id uuid not null references devices(id),
  public_key text not null,
  algorithm text not null default 'Ed25519',
  status text not null default 'active',
  created_at timestamptz not null default now(),
  rotated_at timestamptz null,
  revoked_at timestamptz null,

  constraint device_keys_algorithm_check check (algorithm in ('Ed25519')),
  constraint device_keys_status_check check (status in ('active', 'rotated', 'revoked')),
  constraint device_keys_public_key_not_blank check (length(btrim(public_key)) > 0)
);

create index idx_device_keys_device_status on device_keys(device_id, status);
create index idx_device_keys_app_status on device_keys(tenant_id, app_id, status);

create unique index idx_device_keys_one_active_per_device
on device_keys(device_id)
where status = 'active';
