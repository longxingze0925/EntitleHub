create table team_members (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  email text not null,
  password_hash text not null,
  name text not null,
  phone text null,
  avatar text null,
  status text not null default 'active',
  email_verified boolean not null default false,
  mfa_enabled boolean not null default false,
  mfa_secret_encrypted text null,
  last_login_at timestamptz null,
  last_login_ip inet null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint team_members_status_check check (status in ('active', 'disabled', 'invited'))
);

create unique index idx_team_members_tenant_email
on team_members(tenant_id, lower(email))
where deleted_at is null;

create index idx_team_members_tenant_status
on team_members(tenant_id, status);
