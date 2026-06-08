alter table ai_models
  add column if not exists billing_mode text not null default 'token',
  add column if not exists minute_price_minor bigint not null default 0,
  add column if not exists pricing_config_json jsonb not null default '{}'::jsonb;

update ai_models
set billing_mode = case
    when modality = 'image' then 'per_image'
    when modality = 'video' and request_price_minor > 0 and second_price_minor = 0 then 'video_per_request'
    when modality = 'video' then 'video_per_second'
    when modality = 'audio' and request_price_minor > 0 and second_price_minor = 0 then 'audio_per_request'
    when modality = 'audio' then 'audio_per_second'
    else 'token'
  end
where billing_mode = 'token'
  and modality in ('image', 'video', 'audio');

do $$
begin
  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_models_billing_mode_check'
  ) then
    alter table ai_models
      add constraint ai_models_billing_mode_check
        check (
          billing_mode in (
            'token',
            'per_image',
            'video_per_second',
            'video_per_request',
            'audio_per_second',
            'audio_per_minute',
            'audio_per_request'
          )
        );
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_models_minute_price_nonnegative_check'
  ) then
    alter table ai_models
      add constraint ai_models_minute_price_nonnegative_check
        check (minute_price_minor >= 0);
  end if;

  if not exists (
    select 1
    from pg_constraint
    where conname = 'ai_models_pricing_config_object_check'
  ) then
    alter table ai_models
      add constraint ai_models_pricing_config_object_check
        check (jsonb_typeof(pricing_config_json) = 'object');
  end if;
end $$;
