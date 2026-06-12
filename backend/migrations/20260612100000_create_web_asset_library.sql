create table asset_folders (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  parent_id uuid null references asset_folders(id) on delete set null,
  name text not null,
  metadata_json jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,
  constraint asset_folders_name_not_blank
    check (length(btrim(name)) > 0),
  constraint asset_folders_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object'),
  constraint asset_folders_parent_not_self_check
    check (parent_id is null or parent_id <> id)
);

create unique index idx_asset_folders_active_sibling_name
on asset_folders(
  tenant_id,
  app_id,
  customer_id,
  coalesce(parent_id, '00000000-0000-0000-0000-000000000000'::uuid),
  lower(name)
)
where deleted_at is null;

create index idx_asset_folders_customer_parent
on asset_folders(tenant_id, app_id, customer_id, parent_id, created_at desc)
where deleted_at is null;

create table customer_assets (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  folder_id uuid null references asset_folders(id) on delete set null,
  ai_asset_id uuid null references ai_assets(id) on delete set null,
  name text not null,
  asset_type text not null,
  asset_role text not null,
  source text not null,
  status text not null default 'ready',
  storage_key text null,
  public_url text null,
  mime_type text null,
  file_size bigint null,
  checksum_sha256 text null,
  metadata_json jsonb not null default '{}'::jsonb,
  created_by_server_key_id uuid null references server_api_keys(id) on delete set null,
  deleted_by_server_key_id uuid null references server_api_keys(id) on delete set null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,
  constraint customer_assets_name_not_blank
    check (length(btrim(name)) > 0),
  constraint customer_assets_asset_type_check
    check (asset_type in ('image', 'video', 'audio', 'file')),
  constraint customer_assets_asset_role_check
    check (asset_role in ('upload', 'generated', 'reference', 'first_frame', 'last_frame', 'brand', 'other')),
  constraint customer_assets_source_check
    check (source in ('user_upload', 'generated', 'imported')),
  constraint customer_assets_status_check
    check (status in ('ready', 'deleted')),
  constraint customer_assets_file_size_check
    check (file_size is null or file_size >= 0),
  constraint customer_assets_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object')
);

create unique index idx_customer_assets_ai_asset
on customer_assets(ai_asset_id)
where ai_asset_id is not null;

create index idx_customer_assets_customer_created
on customer_assets(tenant_id, app_id, customer_id, created_at desc)
where deleted_at is null;

create index idx_customer_assets_customer_folder
on customer_assets(tenant_id, app_id, customer_id, folder_id, created_at desc)
where deleted_at is null;

create index idx_customer_assets_type_role
on customer_assets(tenant_id, app_id, customer_id, asset_type, asset_role, created_at desc)
where deleted_at is null;

create table asset_uploads (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  folder_id uuid null references asset_folders(id) on delete set null,
  server_key_id uuid not null references server_api_keys(id) on delete cascade,
  token_hash text not null,
  token_prefix text not null,
  file_name text not null,
  asset_type text not null,
  asset_role text not null,
  mime_type text null,
  file_size bigint null,
  metadata_json jsonb not null default '{}'::jsonb,
  expires_at timestamptz not null,
  consumed_at timestamptz null,
  created_at timestamptz not null default now(),
  constraint asset_uploads_file_name_not_blank
    check (length(btrim(file_name)) > 0),
  constraint asset_uploads_asset_type_check
    check (asset_type in ('image', 'video', 'audio', 'file')),
  constraint asset_uploads_asset_role_check
    check (asset_role in ('upload', 'reference', 'first_frame', 'last_frame', 'brand', 'other')),
  constraint asset_uploads_file_size_check
    check (file_size is null or file_size >= 0),
  constraint asset_uploads_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object')
);

create unique index idx_asset_uploads_token_hash
on asset_uploads(token_hash);

create index idx_asset_uploads_active
on asset_uploads(tenant_id, app_id, customer_id, expires_at)
where consumed_at is null;
