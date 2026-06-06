create table outbox_events (
  id uuid primary key,
  tenant_id uuid null references tenants(id),
  event_type text not null,
  payload jsonb not null,
  status text not null default 'pending',
  attempts int not null default 0,
  next_run_at timestamptz not null default now(),
  last_error text null,
  created_at timestamptz not null default now(),
  processed_at timestamptz null
);

create index idx_outbox_pending
on outbox_events(status, next_run_at)
where status = 'pending';
