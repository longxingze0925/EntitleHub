create table email_settings (
  id boolean primary key default true,
  enabled boolean not null default false,
  smtp_host text not null default '',
  smtp_port int not null default 587,
  smtp_user text null,
  smtp_from text not null default '',
  smtp_password_encrypted text null,
  last_test_status text null,
  last_test_error text null,
  last_test_at timestamptz null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),

  constraint email_settings_singleton check (id),
  constraint email_settings_smtp_port_check check (smtp_port between 1 and 65535),
  constraint email_settings_last_test_status_check check (
    last_test_status is null
    or last_test_status in ('success', 'failed')
  )
);
