insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
cross join permissions p
where r.builtin = true
  and r.code = 'admin'
  and p.code = 'tenant:update'
on conflict do nothing;
