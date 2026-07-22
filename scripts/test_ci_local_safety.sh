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

require_source "spawned Anvil PID is checked" 'kill -0 "\$ANVIL_PID"'
require_source "Anvil binds the explicit loopback port" 'anvil --chain-id 31337 --host 127\.0\.0\.1 --port 8545'
require_source "existing 8545 RPC is rejected" 'port 8545 is already serving an EVM node; refusing to reuse it'
require_source "ICP reuse requires the Bridge CI project marker" 'bridge-ci-project-marker\.json'
require_source "existing test canister is snapshotted" 'snapshot create bridge-canister'
require_source "existing test canister is restored" 'snapshot restore bridge-canister'
require_source "temporary test canister is deleted" 'canister delete bridge-canister'
require_source "spawned Anvil is awaited during cleanup" 'wait "\$ANVIL_PID"'
require_source "snapshot restore failure retains the snapshot" 'failed to restore the Bridge smoke snapshot; snapshot retained'
require_source "cleanup continues through an accumulated failure" 'restore_smoke_canister_state \|\| cleanup_failed=1'
require_source "mapping restoration has an independent failure path" 'failed to restore the local canister mapping'
require_source "failed recovery artifacts are retained" 'recovery artifacts retained at \$TMP_ROOT'
require_source "a canister restarts only after snapshot restore" '"\$ICP_TEST_CANISTER_WAS_RUNNING" -eq 1 && "\$restored" -eq 1'
require_source "cleanup failure changes a successful exit" 'if \[\[ "\$original_status" -eq 0 \]\]; then exit 1'
require_source "owned network stop failure retains ownership" 'failed to stop the Bridge-owned local ICP network; ownership marker retained'

process_marker_count="$(rg -c '"purpose": "bridge-ci-smoke"' "$SOURCE")"
if [[ "$process_marker_count" -ne 2 ]]; then
  echo "ci-local safety regression: marker writer and verifier must use the same purpose" >&2
  exit 1
fi

echo "ci-local process and state isolation tests passed"
