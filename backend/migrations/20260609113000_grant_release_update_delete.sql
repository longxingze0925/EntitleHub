insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in (
    'release:read',
    'release:upload',
    'release:create',
    'release:update',
    'release:publish',
    'release:deprecate',
    'release:delete'
  )
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
