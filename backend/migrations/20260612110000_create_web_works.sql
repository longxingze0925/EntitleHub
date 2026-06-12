create table customer_works (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  owner_customer_id uuid not null references customers(id) on delete cascade,
  source_job_id uuid null references ai_generation_jobs(id) on delete set null,
  title text not null,
  description text null,
  work_type text not null,
  status text not null default 'active',
  visibility text not null default 'private',
  cover_asset_id uuid null references customer_assets(id) on delete set null,
  primary_asset_id uuid not null references customer_assets(id) on delete restrict,
  metadata_json jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,
  constraint customer_works_title_not_blank
    check (length(btrim(title)) > 0),
  constraint customer_works_work_type_check
    check (work_type in ('image', 'video', 'audio', 'file')),
  constraint customer_works_status_check
    check (status in ('active', 'deleted')),
  constraint customer_works_visibility_check
    check (visibility in ('private', 'public')),
  constraint customer_works_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object')
);

create unique index idx_customer_works_active_primary_asset
on customer_works(tenant_id, app_id, primary_asset_id)
where deleted_at is null;

create index idx_customer_works_owner_created
on customer_works(tenant_id, app_id, owner_customer_id, created_at desc)
where deleted_at is null;

create index idx_customer_works_source_job
on customer_works(tenant_id, app_id, source_job_id)
where source_job_id is not null and deleted_at is null;

create table work_assets (
  work_id uuid not null references customer_works(id) on delete cascade,
  asset_id uuid not null references customer_assets(id) on delete cascade,
  role text not null,
  sort_order integer not null default 0,
  created_at timestamptz not null default now(),
  primary key (work_id, asset_id, role),
  constraint work_assets_role_check
    check (role in ('primary', 'cover', 'source', 'reference', 'result'))
);

create index idx_work_assets_asset
on work_assets(asset_id, work_id);

create table work_favorites (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  work_id uuid not null references customer_works(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  created_at timestamptz not null default now(),
  deleted_at timestamptz null
);

create unique index idx_work_favorites_active_unique
on work_favorites(tenant_id, app_id, work_id, customer_id)
where deleted_at is null;

create index idx_work_favorites_customer_created
on work_favorites(tenant_id, app_id, customer_id, created_at desc)
where deleted_at is null;

create table work_publications (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  work_id uuid not null references customer_works(id) on delete cascade,
  status text not null,
  published_at timestamptz null,
  reviewed_by uuid null references team_members(id) on delete set null,
  review_note text null,
  tags jsonb not null default '[]'::jsonb,
  sort_score bigint not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint work_publications_status_check
    check (status in ('draft', 'pending_review', 'published', 'rejected', 'unpublished', 'taken_down')),
  constraint work_publications_tags_array_check
    check (jsonb_typeof(tags) = 'array')
);

create unique index idx_work_publications_work
on work_publications(tenant_id, app_id, work_id);

create index idx_work_publications_gallery
on work_publications(tenant_id, app_id, status, sort_score desc, published_at desc)
where status = 'published';
