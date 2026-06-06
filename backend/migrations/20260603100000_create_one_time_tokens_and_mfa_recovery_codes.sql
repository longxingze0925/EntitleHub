create table admin_mfa_recovery_codes (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  team_member_id uuid not null references team_members(id),
  code_hash text not null,
  used_at timestamptz null,
  revoked_at timestamptz null,
  created_at timestamptz not null default now()
);

create index idx_admin_mfa_recovery_codes_member
on admin_mfa_recovery_codes(team_member_id, used_at, revoked_at);

create table one_time_tokens (
  id uuid primary key,
  tenant_id uuid null references tenants(id),
  purpose text not null,
  subject_type text not null,
  subject_id uuid null,
  email text null,
  token_hash text not null unique,
  created_by uuid null references team_members(id),
  expires_at timestamptz not null,
  consumed_at timestamptz null,
  revoked_at timestamptz null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);

create index idx_one_time_tokens_subject
on one_time_tokens(tenant_id, subject_type, subject_id, purpose);

create index idx_one_time_tokens_active
on one_time_tokens(purpose, expires_at)
where consumed_at is null and revoked_at is null;
