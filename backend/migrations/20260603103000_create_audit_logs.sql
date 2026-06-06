create table audit_logs (
  id uuid primary key,
  tenant_id uuid null references tenants(id),
  actor_type text not null,
  actor_id uuid null,
  action text not null,
  resource_type text not null,
  resource_id uuid null,
  ip inet null,
  user_agent text null,
  request_id text null,
  before_json jsonb null,
  after_json jsonb null,
  metadata_json jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now()
);

create index idx_audit_logs_tenant_time
on audit_logs(tenant_id, created_at desc);

create index idx_audit_logs_resource
on audit_logs(resource_type, resource_id);

create index idx_audit_logs_actor
on audit_logs(actor_type, actor_id);
