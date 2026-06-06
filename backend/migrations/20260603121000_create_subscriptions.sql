create table subscriptions (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  app_id uuid not null references applications(id),
  customer_id uuid not null references customers(id),
  plan text not null,
  status text not null default 'active',
  max_devices int not null default 1,
  features jsonb not null default '[]'::jsonb,
  starts_at timestamptz not null,
  expires_at timestamptz null,
  cancelled_at timestamptz null,
  metadata jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null,

  constraint subscriptions_status_check check (
    status in ('active', 'trialing', 'past_due', 'cancelled', 'expired')
  ),
  constraint subscriptions_plan_not_blank check (length(btrim(plan)) > 0),
  constraint subscriptions_max_devices_check check (max_devices >= 0),
  constraint subscriptions_validity_window_check check (
    expires_at is null or expires_at > starts_at
  )
);

create index idx_subscriptions_customer on subscriptions(customer_id);
create index idx_subscriptions_tenant_app on subscriptions(tenant_id, app_id);
create index idx_subscriptions_status_expires on subscriptions(status, expires_at);

alter table devices
add constraint devices_subscription_id_fkey
foreign key (subscription_id) references subscriptions(id);

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
cross join permissions p
where r.builtin = true
  and r.code = 'admin'
  and p.code in (
    'app:rotate_key',
    'app:read_key',
    'license:suspend',
    'license:renew',
    'license:reset_device',
    'subscription:read',
    'subscription:create',
    'subscription:cancel',
    'device:unblacklist',
    'release:deprecate',
    'script:update',
    'script:deprecate',
    'script:revoke'
  )
on conflict do nothing;
