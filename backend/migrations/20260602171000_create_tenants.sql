create table tenants (
  id uuid primary key,
  name text not null,
  slug text not null,
  status text not null default 'active',
  plan text not null default 'free',
  max_applications int not null default 3,
  max_team_members int not null default 5,
  max_customers int not null default 1000,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint tenants_status_check check (status in ('active', 'suspended', 'deleted')),
  constraint tenants_limits_check check (
    max_applications >= 0
    and max_team_members >= 0
    and max_customers >= 0
  )
);

create unique index idx_tenants_slug
on tenants(slug)
where deleted_at is null;

create index idx_tenants_status
on tenants(status);
