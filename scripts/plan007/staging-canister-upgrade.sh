#!/usr/bin/env bash
# Fail-closed wrapper for a reviewed current-schema staging Bridge upgrade.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec python3 -B "$ROOT/scripts/plan007/staging_canister_upgrade.py" "$@"
