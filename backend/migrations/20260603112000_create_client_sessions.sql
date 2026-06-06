create table client_sessions (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  customer_id uuid null references customers(id),
  device_id uuid not null references devices(id),
  machine_id text not null,
  auth_mode text not null,
  user_agent text null,
  client_ip inet null,
  last_used_at timestamptz null,
  expires_at timestamptz not null,
  revoked_at timestamptz null,
  created_at timestamptz not null default now(),

  constraint client_sessions_auth_mode_check check (auth_mode in ('license', 'subscription', 'both')),
  constraint client_sessions_machine_id_not_blank check (length(btrim(machine_id)) > 0)
);

create index idx_client_sessions_device on client_sessions(device_id);
create index idx_client_sessions_active on client_sessions(id, revoked_at, expires_at);
create index idx_client_sessions_tenant_customer on client_sessions(tenant_id, customer_id);

create table client_refresh_tokens (
  id uuid primary key,
  session_id uuid not null references client_sessions(id),
  token_hash text not null unique,
  created_at timestamptz not null default now(),
  expires_at timestamptz not null,
  used_at timestamptz null,
  revoked_at timestamptz null
);

create index idx_client_refresh_tokens_session on client_refresh_tokens(session_id);
create index idx_client_refresh_tokens_active on client_refresh_tokens(token_hash, revoked_at, expires_at);
