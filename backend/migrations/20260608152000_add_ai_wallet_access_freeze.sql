alter table ai_wallets
  add column if not exists ai_enabled boolean not null default true;
