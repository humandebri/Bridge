#!/usr/bin/env bash
# Fail-closed wrapper for the reviewed Base Sepolia staging RPC replacement.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec python3 "$ROOT/scripts/plan007/staging_canister_upgrade.py" "$@"
