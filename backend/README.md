# Backend

Rust/Axum backend for the user administration platform.

## Prerequisites

- Rust stable toolchain.
- PostgreSQL 17 or compatible PostgreSQL.
- Redis.
- Local object storage directory when `OBJECT_STORAGE_MODE=local`.

## Configuration

Copy `.env.example` to `.env` for local development and replace every secret before using a shared or production environment.

Required local services:

```text
DATABASE_URL=postgres://app_user:app_password@127.0.0.1:5432/user_admin
REDIS_URL=redis://127.0.0.1:6379
OBJECT_STORAGE_MODE=local
OBJECT_STORAGE_LOCAL_ROOT=./storage
```

Security-sensitive values must be unique per environment:

```text
SESSION_SECRET
CSRF_SECRET
TOKEN_HASH_PEPPER
REFRESH_TOKEN_PEPPER
MASTER_KEY
ALERTMANAGER_WEBHOOK_TOKEN
```

Generate secret material with:

```bash
openssl rand -base64 32
```

`MASTER_KEY` protects encrypted private keys and sensitive settings. Back it up; rotating or losing it without a migration plan can make encrypted data unrecoverable.

`ALERTMANAGER_WEBHOOK_TOKEN` protects the internal Alertmanager adapter endpoint. Webhook URLs, SMTP passwords, and PagerDuty routing keys are configured in the Admin UI and encrypted with `MASTER_KEY`; they should not be written into Alertmanager YAML.

Notification channel tests default to dry-run validation. Real delivery tests require explicit confirmation from the Admin UI/API and write audit records plus notification delivery metrics.

## Database

Run migrations against the configured database:

```bash
sqlx migrate run
```

The backend binary can also run embedded migrations:

```bash
user-admin-backend migrate
```

The application enables `pgcrypto` in migrations for UUID generation. Use a disposable database for tests that run migrations.

## Development

Start the backend:

```bash
cargo run
```

Default local API address:

```text
http://127.0.0.1:8080
```

Health endpoints:

```text
GET /health
GET /healthz
GET /readyz
```

Route-level OpenAPI contract:

```text
openapi.yaml
```

The OpenAPI file tracks registered routes and security schemes. Field-level request and response details remain in `../API接口文档.md`.

## Verification

Run the default checks before handing off backend changes:

```bash
cargo fmt --check
cargo test
```

The ignored database integration test for device key rotation requires a disposable PostgreSQL database:

```bash
set DATABASE_URL=postgres://postgres@127.0.0.1:5432/user_admin_test
cargo test --test device_key_rotation -- --ignored
```

On PowerShell:

```powershell
$env:DATABASE_URL="postgres://postgres@127.0.0.1:5432/user_admin_test"
cargo test --test device_key_rotation -- --ignored
```

Do not point ignored tests at a shared or production database.

## Security Notes

- Admin sessions use HttpOnly cookies plus CSRF protection.
- Client protected APIs require bearer session validation and device request signatures.
- Refresh tokens, one-time tokens, license keys, and download tokens are stored as hashes.
- JWKS endpoints expose public keys only.
- All tenant-owned queries must preserve tenant isolation.
- Audit logs must not store plaintext secrets, private keys, raw tokens, or full device public key material unless explicitly required by the API contract.
