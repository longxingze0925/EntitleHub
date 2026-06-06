insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in (
    'role:read',
    'role:create',
    'role:update',
    'role:delete',
    'permission:read'
  )
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
