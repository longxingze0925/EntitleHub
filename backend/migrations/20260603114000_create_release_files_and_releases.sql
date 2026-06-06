create table release_files (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  storage_key text not null,
  file_name text not null,
  file_size bigint not null,
  sha256 text not null,
  signing_key_id uuid not null references signing_keys(id),
  signature_kid text not null,
  signature text not null,
  signature_alg text not null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),

  constraint release_files_size_check check (file_size > 0),
  constraint release_files_sha256_check check (sha256 ~ '^[0-9a-f]{64}$'),
  constraint release_files_signature_alg_check check (signature_alg in ('Ed25519'))
);

create index idx_release_files_app on release_files(tenant_id, app_id);
create index idx_release_files_sha256 on release_files(sha256);

create table releases (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  file_id uuid not null references release_files(id),
  version text not null,
  version_code bigint not null,
  status text not null default 'draft',
  changelog text null,
  force_update boolean not null default false,
  published_at timestamptz null,
  deprecated_at timestamptz null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint releases_version_code_check check (version_code > 0),
  constraint releases_status_check check (status in ('draft', 'published', 'deprecated', 'revoked'))
);

create unique index idx_releases_app_version
on releases(tenant_id, app_id, version)
where deleted_at is null;

create index idx_releases_latest
on releases(tenant_id, app_id, status, version_code desc)
where deleted_at is null;
