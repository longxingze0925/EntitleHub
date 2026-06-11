create table server_api_keys (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  app_id uuid not null references applications(id) on delete cascade,
  name text not null,
  key_prefix text not null,
  key_hash text not null,
  status text not null default 'active',
  scopes jsonb not null default '["ai:invoke"]'::jsonb,
  expires_at timestamptz null,
  last_used_at timestamptz null,
  created_by uuid null references team_members(id) on delete set null,
  created_at timestamptz not null default now(),
  revoked_at timestamptz null,
  constraint server_api_keys_status_check
    check (status in ('active', 'revoked')),
  constraint server_api_keys_name_not_blank
    check (length(btrim(name)) > 0),
  constraint server_api_keys_scopes_array_check
    check (jsonb_typeof(scopes) = 'array')
);

create unique index idx_server_api_keys_hash
on server_api_keys(key_hash);

create index idx_server_api_keys_tenant_app_created
on server_api_keys(tenant_id, app_id, created_at desc);

create index idx_server_api_keys_active
on server_api_keys(key_hash, status)
where status = 'active' and revoked_at is null;

insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'server_api_key:read', '查看服务端密钥', 'server_api_key', 'read'),
  (gen_random_uuid(), 'server_api_key:update', '管理服务端密钥', 'server_api_key', 'update')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in ('server_api_key:read', 'server_api_key:update')
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
