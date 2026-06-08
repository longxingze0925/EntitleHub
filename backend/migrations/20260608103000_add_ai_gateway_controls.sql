alter table ai_wallets
  add column if not exists daily_spend_limit_minor bigint null;

alter table ai_api_keys
  add column if not exists daily_spend_limit_minor bigint null;

alter table ai_models
  add column if not exists daily_spend_limit_minor bigint null;

alter table ai_usage_records
  add column if not exists api_key_id uuid null references ai_api_keys(id) on delete set null,
  add column if not exists idempotency_key text null;

alter table ai_assets
  add column if not exists deleted_at timestamptz null,
  add column if not exists deleted_by uuid null references team_members(id) on delete set null;

alter table ai_assets
  drop constraint if exists ai_assets_status_check;

alter table ai_assets
  add constraint ai_assets_status_check
    check (status in ('caching', 'ready', 'failed', 'deleted'));

do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_wallets_daily_spend_limit_nonnegative_check'
  ) then
    alter table ai_wallets
      add constraint ai_wallets_daily_spend_limit_nonnegative_check
        check (daily_spend_limit_minor is null or daily_spend_limit_minor >= 0);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_api_keys_daily_spend_limit_nonnegative_check'
  ) then
    alter table ai_api_keys
      add constraint ai_api_keys_daily_spend_limit_nonnegative_check
        check (daily_spend_limit_minor is null or daily_spend_limit_minor >= 0);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_models_daily_spend_limit_nonnegative_check'
  ) then
    alter table ai_models
      add constraint ai_models_daily_spend_limit_nonnegative_check
        check (daily_spend_limit_minor is null or daily_spend_limit_minor >= 0);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_usage_idempotency_key_len_check'
  ) then
    alter table ai_usage_records
      add constraint ai_usage_idempotency_key_len_check
        check (idempotency_key is null or length(btrim(idempotency_key)) between 1 and 200);
  end if;
end $$;

create unique index if not exists idx_ai_usage_idempotency
on ai_usage_records(tenant_id, customer_id, api_key_id, endpoint, idempotency_key)
where idempotency_key is not null;

create index if not exists idx_ai_usage_tenant_api_key_created
on ai_usage_records(tenant_id, api_key_id, created_at desc);

create index if not exists idx_ai_usage_tenant_model_created
on ai_usage_records(tenant_id, model_id, created_at desc);

create index if not exists idx_ai_assets_tenant_status_created
on ai_assets(tenant_id, status, created_at desc);

insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'ai:asset:delete', '删除 AI 生成素材', 'ai', 'asset_delete')
on conflict (code) do nothing;

insert into role_permissions (role_id, permission_id)
select r.id, p.id
from roles r
join permissions p
  on p.code = 'ai:asset:delete'
where r.builtin = true
  and r.code in ('owner', 'admin')
on conflict do nothing;
