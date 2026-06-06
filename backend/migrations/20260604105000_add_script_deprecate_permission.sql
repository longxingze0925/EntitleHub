insert into permissions (id, code, name, resource, action)
values (
  gen_random_uuid(),
  'script:deprecate',
  'Deprecate scripts',
  'script',
  'deprecate'
)
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'script:deprecate'
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
