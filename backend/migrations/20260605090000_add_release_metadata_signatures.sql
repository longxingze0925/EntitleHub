alter table releases
  add column signing_key_id uuid null references signing_keys(id),
  add column signature_kid text null,
  add column signature text null,
  add column signature_alg text null;

alter table releases
  add constraint releases_signature_alg_check
  check (signature_alg is null or signature_alg in ('Ed25519'));
