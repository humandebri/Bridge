#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE="$ROOT/scripts/ci-local.sh"

require_source() {
  local description="$1"
  local pattern="$2"
  if ! rg -q -- "$pattern" "$SOURCE"; then
    echo "ci-local safety regression: $description" >&2
    exit 1
  fi
}

require_source "Anvil binds the explicit loopback port" 'anvil --chain-id 31337 --host 127\.0\.0\.1 --port 8545'
require_source "existing 8545 RPC is rejected" 'port 8545 is already serving an EVM node; refusing to reuse it'
require_source "ICP reuse requires the Bridge CI project marker" 'bridge-ci-project-marker\.json'
require_source "existing test canister is snapshotted" 'snapshot create bridge-canister'

process_marker_count="$(rg -c '"purpose": "bridge-ci-smoke"' "$SOURCE")"
if [[ "$process_marker_count" -ne 2 ]]; then
  echo "ci-local safety regression: marker writer and verifier must use the same purpose" >&2
  exit 1
fi

python3 "$ROOT/scripts/test_ci_cleanup.py"
echo "ci-local process and state isolation tests passed"
