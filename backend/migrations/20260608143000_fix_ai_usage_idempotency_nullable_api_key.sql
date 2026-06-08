drop index if exists idx_ai_usage_idempotency;

create unique index if not exists idx_ai_usage_idempotency
on ai_usage_records(
  tenant_id,
  customer_id,
  coalesce(api_key_id, '00000000-0000-0000-0000-000000000000'::uuid),
  endpoint,
  idempotency_key
)
where idempotency_key is not null;
