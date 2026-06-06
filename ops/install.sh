#!/usr/bin/env bash
set -Eeuo pipefail

raw_base_was_set="${USER_ADMIN_RAW_BASE+x}"
USER_ADMIN_REPO="${USER_ADMIN_REPO:-longxingze0925/EntitleHub}"
USER_ADMIN_REF="${USER_ADMIN_REF:-main}"
USER_ADMIN_RAW_BASE="${USER_ADMIN_RAW_BASE:-https://raw.githubusercontent.com/${USER_ADMIN_REPO}/${USER_ADMIN_REF}}"

script_dir=""
if [[ -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd -P || true)"
fi

if [[ -n "$script_dir" && -f "$script_dir/user-adminctl.sh" ]]; then
  exec bash "$script_dir/user-adminctl.sh"
fi

tmp_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT

curl -fsSL "$USER_ADMIN_RAW_BASE/ops/user-adminctl.sh" -o "$tmp_dir/user-adminctl.sh"
exec bash "$tmp_dir/user-adminctl.sh"
