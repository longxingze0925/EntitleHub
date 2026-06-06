create table notification_channels (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  name text not null,
  kind text not null,
  enabled boolean not null default true,
  config_json jsonb not null default '{}'::jsonb,
  secret_encrypted text null,
  last_test_status text null,
  last_test_error text null,
  last_test_at timestamptz null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint notification_channels_kind_check
    check (kind in ('webhook', 'email', 'pagerduty')),
  constraint notification_channels_config_object_check
    check (jsonb_typeof(config_json) = 'object'),
  constraint notification_channels_last_test_status_check
    check (last_test_status is null or last_test_status in ('success', 'failed'))
);

create unique index idx_notification_channels_tenant_name
on notification_channels(tenant_id, lower(name));

create index idx_notification_channels_tenant_kind
on notification_channels(tenant_id, kind);

insert into permissions (id, code, name, resource, action)
values
  (
    gen_random_uuid(),
    'notification:read',
    'Read notification channels',
    'notification',
    'read'
  ),
  (
    gen_random_uuid(),
    'notification:update',
    'Update notification channels',
    'notification',
    'update'
  )
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in ('notification:read', 'notification:update')
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'notification:read'
where r.builtin = true
  and r.code = 'viewer'
on conflict do nothing;
