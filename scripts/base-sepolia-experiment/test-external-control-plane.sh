#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT/scripts/base-sepolia-experiment/experiment.sh"

for stage in schedule resume flow; do
  output="$(
    BASE_SEPOLIA_EXTERNAL_BRIDGE_SIGNER=0x1 "$SCRIPT" "$stage" 2>&1 || true
  )"
  [[ "$output" == *"external control plane is deploy-only; run $stage through the Bridge Canister governance flow"* ]] || {
    echo "$stage did not fail closed for the external control plane" >&2
    exit 1
  }
done

echo "external control-plane stage guard tests passed"
