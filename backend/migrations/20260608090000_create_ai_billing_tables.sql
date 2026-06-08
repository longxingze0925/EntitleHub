create table ai_providers (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  name text not null,
  kind text not null,
  base_url text not null,
  enabled boolean not null default true,
  config_json jsonb not null default '{}'::jsonb,
  secret_encrypted text null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint ai_providers_kind_check
    check (kind in ('openai_compatible', 'custom_http', 'claude', 'gemini', 'deepseek', 'image', 'video')),
  constraint ai_providers_config_object_check
    check (jsonb_typeof(config_json) = 'object')
);

create unique index idx_ai_providers_tenant_name
on ai_providers(tenant_id, lower(name));

create index idx_ai_providers_tenant_kind
on ai_providers(tenant_id, kind);

create table ai_models (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  provider_id uuid null references ai_providers(id) on delete set null,
  code text not null,
  name text not null,
  modality text not null,
  provider_model text null,
  enabled boolean not null default true,
  currency text not null default 'CNY',
  input_1k_price_minor bigint not null default 0,
  output_1k_price_minor bigint not null default 0,
  request_price_minor bigint not null default 0,
  image_price_minor bigint not null default 0,
  second_price_minor bigint not null default 0,
  metadata_json jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint ai_models_modality_check
    check (modality in ('text', 'image', 'video', 'audio', 'embedding', 'multimodal')),
  constraint ai_models_currency_check
    check (currency ~ '^[A-Z]{3}$'),
  constraint ai_models_prices_nonnegative_check
    check (
      input_1k_price_minor >= 0
      and output_1k_price_minor >= 0
      and request_price_minor >= 0
      and image_price_minor >= 0
      and second_price_minor >= 0
    ),
  constraint ai_models_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object')
);

create unique index idx_ai_models_tenant_code
on ai_models(tenant_id, lower(code));

create index idx_ai_models_tenant_modality
on ai_models(tenant_id, modality);

create index idx_ai_models_tenant_provider
on ai_models(tenant_id, provider_id);

create table ai_wallets (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  currency text not null default 'CNY',
  balance_minor bigint not null default 0,
  held_minor bigint not null default 0,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint ai_wallets_currency_check
    check (currency ~ '^[A-Z]{3}$'),
  constraint ai_wallets_balance_nonnegative_check
    check (balance_minor >= 0 and held_minor >= 0 and held_minor <= balance_minor)
);

create unique index idx_ai_wallets_tenant_customer
on ai_wallets(tenant_id, customer_id);

create table ai_wallet_ledger_entries (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  wallet_id uuid not null references ai_wallets(id) on delete cascade,
  customer_id uuid not null references customers(id) on delete cascade,
  entry_type text not null,
  amount_minor bigint not null,
  balance_after_minor bigint not null,
  held_after_minor bigint not null,
  reason text not null,
  reference_type text null,
  reference_id text null,
  metadata_json jsonb not null default '{}'::jsonb,
  created_by uuid null references team_members(id) on delete set null,
  created_at timestamptz not null default now(),
  constraint ai_wallet_ledger_entry_type_check
    check (entry_type in ('credit', 'debit', 'hold', 'capture', 'release', 'refund', 'adjustment')),
  constraint ai_wallet_ledger_amount_nonzero_check
    check (amount_minor <> 0),
  constraint ai_wallet_ledger_after_nonnegative_check
    check (balance_after_minor >= 0 and held_after_minor >= 0 and held_after_minor <= balance_after_minor),
  constraint ai_wallet_ledger_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object')
);

create index idx_ai_wallet_ledger_tenant_customer_created
on ai_wallet_ledger_entries(tenant_id, customer_id, created_at desc);

create index idx_ai_wallet_ledger_wallet_created
on ai_wallet_ledger_entries(wallet_id, created_at desc);

create table ai_usage_records (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  wallet_id uuid null references ai_wallets(id) on delete set null,
  customer_id uuid null references customers(id) on delete set null,
  provider_id uuid null references ai_providers(id) on delete set null,
  model_id uuid null references ai_models(id) on delete set null,
  request_id text null,
  endpoint text not null,
  status text not null,
  provider_status text null,
  provider_request_id text null,
  prompt_tokens bigint null,
  completion_tokens bigint null,
  total_tokens bigint null,
  charged_minor bigint not null default 0,
  refunded_minor bigint not null default 0,
  provider_cost_minor bigint null,
  price_snapshot_json jsonb not null default '{}'::jsonb,
  provider_raw_response jsonb null,
  metadata_json jsonb not null default '{}'::jsonb,
  created_at timestamptz not null default now(),
  completed_at timestamptz null,
  constraint ai_usage_status_check
    check (status in ('pending', 'running', 'succeeded', 'failed', 'refunded')),
  constraint ai_usage_amounts_nonnegative_check
    check (charged_minor >= 0 and refunded_minor >= 0 and (provider_cost_minor is null or provider_cost_minor >= 0)),
  constraint ai_usage_price_snapshot_object_check
    check (jsonb_typeof(price_snapshot_json) = 'object'),
  constraint ai_usage_metadata_object_check
    check (jsonb_typeof(metadata_json) = 'object')
);

create index idx_ai_usage_tenant_created
on ai_usage_records(tenant_id, created_at desc);

create index idx_ai_usage_tenant_customer_created
on ai_usage_records(tenant_id, customer_id, created_at desc);

create table ai_assets (
  id uuid primary key default gen_random_uuid(),
  tenant_id uuid not null references tenants(id) on delete cascade,
  usage_id uuid null references ai_usage_records(id) on delete set null,
  asset_type text not null,
  status text not null,
  provider_url text null,
  storage_key text null,
  public_url text null,
  mime_type text null,
  file_size bigint null,
  checksum_sha256 text null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  constraint ai_assets_asset_type_check
    check (asset_type in ('image', 'video', 'audio', 'file')),
  constraint ai_assets_status_check
    check (status in ('caching', 'ready', 'failed')),
  constraint ai_assets_file_size_check
    check (file_size is null or file_size >= 0)
);

create index idx_ai_assets_tenant_created
on ai_assets(tenant_id, created_at desc);

create index idx_ai_assets_usage
on ai_assets(usage_id);

insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'ai:read', '查看 AI 计费', 'ai', 'read'),
  (gen_random_uuid(), 'ai:provider:update', '管理 AI 渠道', 'ai', 'provider_update'),
  (gen_random_uuid(), 'ai:model:update', '管理 AI 模型价格', 'ai', 'model_update'),
  (gen_random_uuid(), 'ai:wallet:update', '调整 AI 钱包余额', 'ai', 'wallet_update')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code in ('ai:read', 'ai:provider:update', 'ai:model:update', 'ai:wallet:update')
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'ai:read'
where r.builtin = true
  and r.code in ('developer', 'viewer')
on conflict do nothing;
