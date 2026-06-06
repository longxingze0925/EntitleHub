insert into permissions (id, code, name, resource, action)
values (
  gen_random_uuid(),
  'security:retry_event',
  'Retry outbox events',
  'security',
  'retry_event'
)
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in ('security:view_events', 'security:retry_event')
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
