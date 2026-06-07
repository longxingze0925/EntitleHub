# Operations

This directory contains first-run deployment and backup helpers.

## One-command Installer

Linux hosts can use the interactive one-command installer:

```bash
bash <(curl -Ls https://raw.githubusercontent.com/longxingze0925/EntitleHub/main/ops/install.sh)
```

The command opens a menu for install, update, uninstall, status, logs, backup, restore, certificate management, and diagnostics. Install mode supports local-only access, server-IP access, domain with automatic HTTPS certificate, domain with a custom certificate, and deployment behind an existing reverse proxy. Updates keep `.env.compose`, certificates, backups, and Docker volumes, then refresh source files, rebuild, migrate, restart, and run smoke checks.

## Docker Compose

Create the local compose environment file:

```powershell
pwsh -File ops/new-compose-env.ps1
```

The script generates local-only passwords and secrets, including a valid 32-byte base64 `MASTER_KEY` and `ALERTMANAGER_WEBHOOK_TOKEN`.

If you prefer to create the file manually, copy the example and replace every password and secret in `.env.compose`:

```bash
cp .env.compose.example .env.compose
openssl rand -base64 32
```

Host ports can be changed in `.env.compose` when another local stack already uses the defaults:

```text
COMPOSE_HOST_BIND=127.0.0.1
BACKEND_HOST_PORT=18080
REDIS_HOST_PORT=16379
GRAFANA_HOST_PORT=13000
```

Set `COMPOSE_HOST_BIND=0.0.0.0` only when another machine must access the local compose stack.

Start infrastructure first:

```bash
docker compose -p user-admin --env-file .env.compose up -d postgres redis
```

Run database migrations from the repository root or from `backend/` with `DATABASE_URL` pointing at the compose PostgreSQL service:

```bash
cd backend
DATABASE_URL=postgres://app_user:password@127.0.0.1:5432/user_admin sqlx migrate run
```

Or run embedded migrations through the backend container:

```bash
docker compose -p user-admin --env-file .env.compose run --rm backend user-admin-backend migrate
```

Then start the application stack:

```bash
docker compose -p user-admin --env-file .env.compose up -d --build
```

Run smoke checks after the stack is up:

```powershell
pwsh -File ops/smoke-compose.ps1
```

After `ops/smoke-init-owner.ps1 -RunMigrations` has created or verified local owner credentials, run the Client SDK live backend smoke:

```powershell
pwsh -File ops/smoke-client-sdk.ps1
```

The script logs in as the local owner, reuses or creates a `sdk-smoke` application, creates a short-lived test license, and runs the ignored SDK live test against the running backend. It does not print owner credentials or the generated license key.

Run the expiry flow smoke to verify active sessions are rejected after license and subscription expiry:

```powershell
pwsh -File ops/smoke-expiry-flow.ps1
```

The script logs in as the local owner, creates a `both`-mode smoke application and a disposable customer, verifies license activation and subscription login before expiry, waits for each entitlement to expire, then verifies client refresh fails with `license_expired` and `subscription_inactive`.

Local URLs:

```text
Admin:   http://localhost:5173/
Backend: http://localhost:8080/
Metrics: http://localhost:8080/metrics
Prometheus: http://localhost:9090/
Alertmanager: http://localhost:9093/
Grafana: http://localhost:3000/
PostgreSQL exporter: http://localhost:9187/metrics
Redis exporter: http://localhost:9121/metrics
```

If host ports were changed in `.env.compose`, use those host port values for browser and local CLI access. Container-to-container URLs keep using the internal service ports. By default, compose binds published ports to `127.0.0.1`.

Prometheus and Grafana are included as first-run monitoring services. The compose defaults use explicit image versions for Prometheus, Alertmanager, Grafana, and exporters. For production, set `GRAFANA_ADMIN_PASSWORD` from your deployment secret store, and override the `*_IMAGE` values with reviewed versions or image digests.

PostgreSQL and Redis exporters are included for dependency-level metrics. Compose healthchecks cover PostgreSQL, Redis, Backend, Admin, Prometheus, Alertmanager, Grafana, and postgres-exporter. The redis-exporter image does not include a shell, so `ops/smoke-compose.ps1` verifies it through Prometheus active targets.

Backend metrics include HTTP counters and latency, dependency health counters, worker failure counters, and Alertmanager notification delivery counters/latency under `notification_delivery_*`.

Alertmanager starts with a no-op receiver so the local stack can boot without a real notification endpoint. For production, prefer routing Alertmanager to the backend adapter at `/api/internal/alertmanager/webhook`; the backend then reads the notification channels configured in the Admin UI and keeps webhook URLs, SMTP passwords, and PagerDuty routing keys encrypted in the database.

The Admin UI notification channel test defaults to dry-run validation. The real delivery test is a separate confirmed action and records audit data plus `notification_delivery_*` metrics.

Render an Alertmanager receiver config from environment-injected values:

```powershell
pwsh -File ops/render-alertmanager-config.ps1 -Mode backend -Check
pwsh -File ops/render-alertmanager-config.ps1 -Mode webhook -WebhookUrl http://alert-receiver.example.internal/alerts -Check
pwsh -File ops/render-alertmanager-config.ps1 -Mode email -Check
pwsh -File ops/render-alertmanager-config.ps1 -Mode production -Check
```

By default, the script writes to `.tools/alertmanager.generated.yml`. Set `-OutputPath ops/alertmanager/alertmanager.yml` only when intentionally replacing the active compose Alertmanager config. Backend mode reads `ALERTMANAGER_WEBHOOK_TOKEN` and defaults to `http://backend:8080/api/internal/alertmanager/webhook`; override with `ALERTMANAGER_BACKEND_WEBHOOK_URL` when needed. Email and production modes read `ALERTMANAGER_SMTP_*`, `ALERTMANAGER_EMAIL_TO`, `ALERTMANAGER_CRITICAL_*`, and `ALERTMANAGER_WARNING_EMAIL_TO` values from the environment when parameters are omitted.

For local compose, prefer the activation helper so the backend-mode Alertmanager config is written under ignored `.tools/` instead of `ops/alertmanager/alertmanager.yml`:

```powershell
pwsh -File ops/activate-alertmanager-backend.ps1 -Check -Restart
```

The helper ensures `ALERTMANAGER_WEBHOOK_TOKEN` exists in `.env.compose`, renders `.tools/alertmanager.backend.yml`, sets `ALERTMANAGER_CONFIG_PATH`, and optionally recreates the Alertmanager container.

Run a controlled end-to-end backend receiver smoke after activation:

```powershell
pwsh -File ops/smoke-alertmanager-backend.ps1
```

The smoke refuses to send when enabled notification channels already exist unless `-AllowConfiguredChannels` is passed. This prevents accidental outbound webhook, SMTP, or PagerDuty notifications during local verification.

## Validation

Run the local CI-style validation before handing off a release:

```powershell
pwsh -File ops/validate-ci.ps1
```

The script checks OpenAPI refs, YAML/JSON assets, backend tests, Client SDK tests, Admin lint/build, Docker Compose config, and optional Prometheus/Alertmanager semantic checks when `promtool` and `amtool` are installed.

When installed, `cargo-audit` and `cargo-deny` are also run. `cargo-audit` uses `.tools/advisory-db`; `cargo-deny` uses `deny.toml` and checks dependency bans, licenses, and sources for both backend and Client SDK manifests.

Run the stricter release gate before production deployment:

```powershell
pwsh -File ops/validate-release-strict.ps1
```

The strict release gate runs `ops/validate-ci.ps1 -StrictExternalTools`, then additionally checks cargo-deny advisories, verifies the registered `RUSTSEC-2023-0071` exception is not present in the backend normal/build dependency tree, and runs full Admin `npm audit` including dev tooling.

Install the Rust audit tools on a workstation:

```powershell
cargo install cargo-audit --locked
cargo install cargo-deny --locked
```

Use `ops/smoke-compose.ps1` for a running compose stack. It checks Backend health/readiness/metrics, Admin, Prometheus, Alertmanager, Grafana, and Prometheus active targets.

Use `ops/smoke-client-sdk.ps1` after `ops/smoke-init-owner.ps1 -RunMigrations` to verify the real SDK activation, refresh, JWKS validation, and heartbeat flow against the compose backend.

Use `ops/smoke-expiry-flow.ps1` against a running compose stack to verify license and subscription expiry behavior through real admin/client APIs.

Use `ops/check-compose-image-pins.ps1` to verify compose service images use explicit tags and do not fall back to `latest`:

```powershell
pwsh -File ops/check-compose-image-pins.ps1
pwsh -File ops/check-compose-image-pins.ps1 -RequireDigest
```

The stricter `-RequireDigest` mode is intended for production release review after image architecture and registry policy are known.

Generate a production digest override from the current compose image set:

```powershell
pwsh -File ops/pin-compose-digests.ps1 -EnvFile .env.compose.example -OutputFile compose.digests.yaml -Pull
docker compose -f compose.yaml -f compose.digests.yaml --env-file .env.compose up -d
```

The generated `compose.digests.yaml` is ignored by Git and overrides image-based services with immutable `@sha256:` references. Built local services such as Backend and Admin are intentionally skipped.

GitHub Actions runs the same script from `.github/workflows/ci.yml` and adds container-based `promtool`/`amtool` checks for Prometheus and Alertmanager configs.

Use these switches when a local machine lacks optional tools:

```powershell
pwsh -File ops/validate-ci.ps1 -SkipDocker
pwsh -File ops/validate-ci.ps1 -SkipFrontend
pwsh -File ops/validate-ci.ps1 -StrictExternalTools
```

Install local `promtool` and `amtool` binaries into `.tools/bin` when semantic monitoring checks are required on a workstation:

```powershell
pwsh -File ops/install-monitoring-tools.ps1
pwsh -File ops/validate-ci.ps1 -StrictExternalTools
```

Initialize the first owner after migrations:

```bash
docker compose -p user-admin --env-file .env.compose run --rm backend user-admin-backend init-owner
```

Run the local init-owner and first-login smoke check:

```powershell
pwsh -File ops/smoke-init-owner.ps1 -RunMigrations
```

The script applies migrations when `-RunMigrations` is set, creates the first owner only when no active tenant exists, verifies `/api/auth/login` and `/api/auth/me`, and stores generated local smoke credentials in `.tools/init-owner-smoke.env`.

## Backup

The backup script requires `pg_dump` and `sha256sum`.

```bash
DATABASE_URL=postgres://app_user:password@127.0.0.1:5432/user_admin ./ops/backup-postgres.sh
```

Backups are written to `./backups` by default.

Run a local backup/restore drill against the running compose PostgreSQL service:

```powershell
pwsh -File ops/drill-postgres-backup-restore.ps1
```

The drill creates disposable source and restore databases, verifies a `pg_dump`/`pg_restore` round trip, writes the dump and SHA256 file to `./backups/drills`, and drops the disposable databases by default. It does not restore into the application database.

Run a local object-storage backup/restore drill against the compose `object-storage` volume:

```powershell
pwsh -File ops/drill-object-storage-backup-restore.ps1
```

The drill archives the source volume, restores it into a disposable Docker volume, compares file SHA256 manifests, writes the archive and SHA256 file to `./backups/object-storage-drills`, and drops the disposable restore volume by default.

Check that `MASTER_KEY` is valid and record a non-secret fingerprint:

```powershell
pwsh -File ops/check-master-key-backup.ps1
```

To verify an offline backup copy, pass a file that contains either the raw key value or `MASTER_KEY=...`:

```powershell
pwsh -File ops/check-master-key-backup.ps1 -BackupKeyFile D:\offline\MASTER_KEY.txt
pwsh -File ops/check-master-key-backup.ps1 -BackupKeyFile D:\offline\MASTER_KEY.txt -RequireBackup
```

The script writes only SHA256 fingerprints to `./backups/master-key-fingerprints`; it does not copy the key material into the backup directory.

## Restore

The restore script requires `pg_restore`.

```bash
DATABASE_URL=postgres://app_user:password@127.0.0.1:5432/user_admin BACKUP_FILE=./backups/user_admin_20260605T120000Z.dump ./ops/restore-postgres.sh
```

Set `DROP_OBJECTS=true` only for an intentional destructive restore into a disposable or prepared database.
