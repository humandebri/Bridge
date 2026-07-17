#!/usr/bin/env bash
# Fixed SNS/Canister activation entrypoint. This driver never signs an EVM transaction.
set -Eeuo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing Gate B evidence hash}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_ACTIVATION_PHASE:?set BRIDGE_ACTIVATION_PHASE=schedule or execute}"

[[ "$BRIDGE_ACTIVATION_PHASE" == schedule || "$BRIDGE_ACTIVATION_PHASE" == execute ]] || {
  echo "invalid activation phase" >&2
  exit 1
}
for tool in python3; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

"$SOURCE_ROOT/scripts/production-live-preflight.sh" verify "$BRIDGE_RELEASE_BUNDLE"
production_validate_gate gate-b "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256"

echo "preflight complete: submit the fixed ${BRIDGE_ACTIVATION_PHASE}_activation SNS function; direct identity calls and EVM wallet sends are intentionally unavailable" >&2
