create table admin_refresh_tokens (
  id uuid primary key,
  session_id uuid not null references admin_sessions(id),
  token_hash text not null unique,
  created_at timestamptz not null default now(),
  expires_at timestamptz not null,
  used_at timestamptz null,
  revoked_at timestamptz null
);

create index idx_admin_refresh_tokens_session on admin_refresh_tokens(session_id);
create index idx_admin_refresh_tokens_active on admin_refresh_tokens(token_hash, revoked_at, expires_at);
