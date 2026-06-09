insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'subscription:suspend', '暂停订阅', 'subscription', 'suspend'),
  (gen_random_uuid(), 'subscription:resume', '恢复订阅', 'subscription', 'resume'),
  (gen_random_uuid(), 'subscription:reset_device', '重置订阅设备', 'subscription', 'reset_device')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in (
    'subscription:read',
    'subscription:create',
    'subscription:update',
    'subscription:cancel',
    'subscription:renew',
    'subscription:suspend',
    'subscription:resume',
    'subscription:reset_device'
  )
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
