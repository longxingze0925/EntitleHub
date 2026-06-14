alter table customer_assets
drop constraint if exists customer_assets_status_check;

alter table customer_assets
add constraint customer_assets_status_check
check (status in ('uploading', 'processing', 'ready', 'failed', 'deleted'));

create index if not exists idx_customer_assets_processing
on customer_assets(status, created_at)
where deleted_at is null
  and status = 'processing'
  and asset_type = 'video';
