create table secure_scripts (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  name text not null,
  version text not null,
  version_code bigint not null,
  status text not null default 'draft',
  content_ciphertext text not null,
  content_sha256 text not null,
  signing_key_id uuid not null references signing_keys(id),
  signature_kid text not null,
  signature text not null,
  signature_alg text not null,
  required_features jsonb not null default '[]'::jsonb,
  expires_at timestamptz null,
  published_at timestamptz null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint secure_scripts_version_code_check check (version_code > 0),
  constraint secure_scripts_status_check check (status in ('draft', 'published', 'deprecated', 'revoked')),
  constraint secure_scripts_sha256_check check (content_sha256 ~ '^[0-9a-f]{64}$'),
  constraint secure_scripts_signature_alg_check check (signature_alg in ('Ed25519')),
  constraint secure_scripts_required_features_check check (jsonb_typeof(required_features) = 'array')
);

create index idx_secure_scripts_app_status
on secure_scripts(tenant_id, app_id, status, version_code desc)
where deleted_at is null;
