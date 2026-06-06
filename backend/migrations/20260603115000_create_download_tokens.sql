create table download_tokens (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  device_id uuid not null references devices(id),
  file_id uuid not null references release_files(id),
  token_hash text not null unique,
  kind text not null,
  expires_at timestamptz not null,
  used_at timestamptz null,
  revoked_at timestamptz null,
  created_at timestamptz not null default now(),

  constraint download_tokens_kind_check check (kind in ('release_file')),
  constraint download_tokens_expiry_check check (expires_at > created_at)
);

create index idx_download_tokens_file_device
on download_tokens(tenant_id, app_id, device_id, file_id);

create index idx_download_tokens_expiry
on download_tokens(expires_at)
where revoked_at is null;
