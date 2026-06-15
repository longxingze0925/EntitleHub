alter table ai_generation_jobs
  drop constraint if exists ai_generation_jobs_job_type_check;

alter table ai_generation_jobs
  add constraint ai_generation_jobs_job_type_check
  check (job_type in ('image', 'video', 'audio'));

alter table ai_generation_jobs
  drop constraint if exists ai_generation_jobs_charge_mode_check;

alter table ai_generation_jobs
  add constraint ai_generation_jobs_charge_mode_check
  check (
    charge_mode in (
      'per_image',
      'video_per_second',
      'video_per_request',
      'audio_per_second',
      'audio_per_minute',
      'audio_per_request'
    )
  );
