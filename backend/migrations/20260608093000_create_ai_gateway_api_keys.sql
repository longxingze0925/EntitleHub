create table ai_api_keys (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  name text not null,
  key_prefix text not null,
  key_hash text not null,
  status text not null default 'active',
  expires_at timestamptz null,
  last_used_at timestamptz null,
  created_by uuid null references team_members(id) on delete set null,
  created_at timestamptz not null default now(),
  revoked_at timestamptz null,
  constraint ai_api_keys_status_check
    check (status in ('active', 'revoked')),
  constraint ai_api_keys_name_not_blank
    check (length(btrim(name)) > 0)
);

create unique index idx_ai_api_keys_hash
on ai_api_keys(key_hash);

create index idx_ai_api_keys_tenant_customer_created
on ai_api_keys(tenant_id, customer_id, created_at desc);

create index idx_ai_api_keys_active
on ai_api_keys(key_hash, status)
where status = 'active' and revoked_at is null;

insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'ai:api_key:update', '管理 AI API Key', 'ai', 'api_key_update')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'ai:api_key:update'
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
