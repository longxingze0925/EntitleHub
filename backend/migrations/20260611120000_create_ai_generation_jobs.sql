alter table ai_providers
  drop constraint if exists ai_providers_kind_check;

alter table ai_providers
  add constraint ai_providers_kind_check
    check (
      kind in (
        'openai_compatible',
        'custom_http',
        'claude',
        'gemini',
        'deepseek',
        'image',
        'video',
        'wuyin_keji'
      )
    );

create table ai_generation_jobs (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  wallet_id uuid null references ai_wallets(id) on delete set null,
  customer_id uuid not null references customers(id) on delete cascade,
  provider_id uuid null references ai_providers(id) on delete set null,
  model_id uuid null references ai_models(id) on delete set null,
  usage_id uuid null references ai_usage_records(id) on delete set null,
  server_key_id uuid null references server_api_keys(id) on delete set null,
  request_id text null,
  idempotency_key text null,
  job_type text not null,
  status text not null,
  provider_status text null,
  provider_job_id text null,
  provider_request_id text null,
  provider_submit_response jsonb null,
  provider_result_response jsonb null,
  request_payload jsonb not null default '{}'::jsonb,
  result_json jsonb null,
  asset_urls jsonb not null default '[]'::jsonb,
  charge_mode text not null,
  quantity bigint not null default 1,
  held_minor bigint not null default 0,
  charged_minor bigint not null default 0,
  refunded_minor bigint not null default 0,
  currency text not null default 'CNY',
  failure_reason text null,
  attempts integer not null default 0,
  next_poll_at timestamptz null,
  submitted_at timestamptz null,
  completed_at timestamptz null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint ai_generation_jobs_job_type_check
    check (job_type in ('image', 'video')),
  constraint ai_generation_jobs_status_check
    check (
      status in (
        'pending',
        'submitted',
        'running',
        'provider_succeeded',
        'caching',
        'succeeded',
        'provider_failed',
        'failed',
        'timeout_review',
        'cancelled'
      )
    ),
  constraint ai_generation_jobs_charge_mode_check
    check (charge_mode in ('per_image', 'video_per_second', 'video_per_request')),
  constraint ai_generation_jobs_currency_check
    check (currency ~ '^[A-Z]{3}$'),
  constraint ai_generation_jobs_amounts_nonnegative_check
    check (
      quantity > 0
      and held_minor >= 0
      and charged_minor >= 0
      and refunded_minor >= 0
    ),
  constraint ai_generation_jobs_payload_object_check
    check (jsonb_typeof(request_payload) = 'object'),
  constraint ai_generation_jobs_asset_urls_array_check
    check (jsonb_typeof(asset_urls) = 'array')
);

create unique index idx_ai_generation_jobs_idempotency
on ai_generation_jobs(tenant_id, customer_id, server_key_id, job_type, idempotency_key)
where idempotency_key is not null;

create index idx_ai_generation_jobs_tenant_created
on ai_generation_jobs(tenant_id, created_at desc);

create index idx_ai_generation_jobs_tenant_status_next_poll
on ai_generation_jobs(tenant_id, status, next_poll_at);

create index idx_ai_generation_jobs_usage
on ai_generation_jobs(usage_id);

insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'ai:job:read', '查看 AI 生成任务', 'ai', 'job_read')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'ai:job:read'
where r.builtin = true
  and r.code in ('owner', 'admin', 'developer', 'viewer')
on conflict do nothing;
