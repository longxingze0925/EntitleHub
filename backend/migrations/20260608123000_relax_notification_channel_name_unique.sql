drop index if exists idx_notification_channels_tenant_name;

create unique index if not exists idx_notification_channels_tenant_active_name
on notification_channels(tenant_id, lower(name))
where enabled = true;
