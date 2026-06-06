#!/usr/bin/env sh
set -eu

if [ -z "${DATABASE_URL:-}" ]; then
  echo "DATABASE_URL is required" >&2
  exit 1
fi

if ! command -v pg_dump >/dev/null 2>&1; then
  echo "pg_dump is required" >&2
  exit 1
fi

if ! command -v sha256sum >/dev/null 2>&1; then
  echo "sha256sum is required" >&2
  exit 1
fi

BACKUP_DIR="${BACKUP_DIR:-./backups}"
BACKUP_PREFIX="${BACKUP_PREFIX:-user_admin}"
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
TARGET="${BACKUP_DIR}/${BACKUP_PREFIX}_${TIMESTAMP}.dump"

mkdir -p "$BACKUP_DIR"
pg_dump --format=custom --no-owner --no-acl "$DATABASE_URL" --file "$TARGET"
sha256sum "$TARGET" > "${TARGET}.sha256"

echo "$TARGET"
