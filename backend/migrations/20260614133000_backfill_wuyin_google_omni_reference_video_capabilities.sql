update ai_models
set
  pricing_config_json = pricing_config_json || jsonb_build_object(
    'capabilities',
    coalesce(pricing_config_json->'capabilities', '{}'::jsonb) || '{
      "resolutions": ["1280x720", "720x1280", "1920x1080", "1080x1920"],
      "durations": [10],
      "default_duration_seconds": 10,
      "inputModes": ["text", "image", "video"],
      "maxReferenceImages": 7,
      "maxReferenceVideos": 1,
      "supportsReferenceVideo": true,
      "supportsFirstFrame": false,
      "supportsLastFrame": false,
      "acceptedMimeTypes": ["image/png", "image/jpeg", "image/webp", "video/mp4"],
      "maxAssetSizeMb": 50
    }'::jsonb
  ),
  updated_at = now()
where
  modality = 'video'
  and (
    lower(coalesce(provider_model, '')) in ('google_omni', 'video_google_omni')
    or lower(code) in ('google_omni', 'video_google_omni')
  );
