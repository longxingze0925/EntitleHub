create table admin_sessions (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  team_member_id uuid not null references team_members(id),
  user_agent text null,
  ip inet null,
  created_at timestamptz not null default now(),
  last_seen_at timestamptz null,
  expires_at timestamptz not null,
  revoked_at timestamptz null
);

create index idx_admin_sessions_member
on admin_sessions(team_member_id);

create index idx_admin_sessions_active
on admin_sessions(team_member_id, revoked_at, expires_at);
