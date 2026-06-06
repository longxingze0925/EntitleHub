create table applications (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  name text not null,
  slug text null,
  app_key text not null,
  app_secret_hash text not null,
  auth_mode text not null default 'both',
  status text not null default 'active',
  heartbeat_interval_seconds int not null default 3600,
  offline_tolerance_seconds int not null default 86400,
  max_devices_default int not null default 1,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint applications_auth_mode_check check (auth_mode in ('license', 'subscription', 'both')),
  constraint applications_status_check check (status in ('active', 'disabled', 'archived')),
  constraint applications_heartbeat_interval_check check (heartbeat_interval_seconds > 0),
  constraint applications_offline_tolerance_check check (offline_tolerance_seconds >= heartbeat_interval_seconds),
  constraint applications_max_devices_default_check check (max_devices_default >= 0)
);

create unique index idx_applications_app_key on applications(app_key);

create unique index idx_applications_tenant_slug
on applications(tenant_id, lower(slug))
where slug is not null and deleted_at is null;

create index idx_applications_tenant_status
on applications(tenant_id, status);

create table signing_keys (
  id uuid primary key,
  tenant_id uuid null references tenants(id),
  app_id uuid null references applications(id),
  key_scope text not null,
  kid text not null unique,
  alg text not null default 'EdDSA',
  public_key_pem text not null,
  private_key_envelope jsonb null,
  status text not null default 'active',
  not_before timestamptz not null default now(),
  not_after timestamptz null,
  rotated_from_id uuid null references signing_keys(id),
  created_by uuid null references team_members(id),
  created_at timestamptz not null default now(),
  activated_at timestamptz null,
  retired_at timestamptz null,
  revoked_at timestamptz null,

  constraint signing_keys_scope_check check (key_scope in ('jwt_access_token', 'release_file', 'secure_script', 'app_request')),
  constraint signing_keys_alg_check check (alg in ('EdDSA')),
  constraint signing_keys_status_check check (status in ('active', 'retiring', 'retired', 'revoked')),
  constraint signing_keys_window_check check (not_after is null or not_after > not_before)
);

create index idx_signing_keys_scope_status
on signing_keys(key_scope, status, not_before, not_after);

create index idx_signing_keys_app_scope
on signing_keys(tenant_id, app_id, key_scope, status);
