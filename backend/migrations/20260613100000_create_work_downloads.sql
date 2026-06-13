create table work_downloads (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  work_id uuid not null references customer_works(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  download_count bigint not null default 1,
  first_downloaded_at timestamptz not null default now(),
  downloaded_at timestamptz not null default now(),
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint work_downloads_count_check
    check (download_count > 0)
);

create unique index idx_work_downloads_unique
on work_downloads(tenant_id, app_id, work_id, customer_id);

create index idx_work_downloads_customer_downloaded
on work_downloads(tenant_id, app_id, customer_id, downloaded_at desc);
