#!/usr/bin/env sh
set -eu

if [ -z "${DATABASE_URL:-}" ]; then
  echo "DATABASE_URL is required" >&2
  exit 1
fi

if [ -z "${BACKUP_FILE:-}" ]; then
  echo "BACKUP_FILE is required" >&2
  exit 1
fi

if [ ! -f "$BACKUP_FILE" ]; then
  echo "backup file not found: $BACKUP_FILE" >&2
  exit 1
fi

if ! command -v pg_restore >/dev/null 2>&1; then
  echo "pg_restore is required" >&2
  exit 1
fi

if [ -f "${BACKUP_FILE}.sha256" ] && command -v sha256sum >/dev/null 2>&1; then
  sha256sum -c "${BACKUP_FILE}.sha256"
fi

RESTORE_FLAGS="--no-owner --no-acl"
if [ "${DROP_OBJECTS:-false}" = "true" ]; then
  RESTORE_FLAGS="$RESTORE_FLAGS --clean --if-exists"
fi

pg_restore $RESTORE_FLAGS --dbname "$DATABASE_URL" "$BACKUP_FILE"
