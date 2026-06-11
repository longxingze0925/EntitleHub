insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'ai:job:update', '处理 AI 生成任务', 'ai', 'job_update')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'ai:job:update'
where r.builtin = true
  and r.code in ('owner', 'admin', 'developer')
on conflict do nothing;
