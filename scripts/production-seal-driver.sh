#!/usr/bin/env bash
# Authorize and perform the one-time production controller operational-config seal.
set -Eeuo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing pre-seal Gate B evidence hash}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_PRODUCTION_CONTROLLER_PEM:?missing production controller identity PEM}"
: "${BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:?missing seal receipt output path}"
: "${BRIDGE_CONFIRM_OPERATIONAL_CONFIG_SEAL:?missing explicit seal confirmation}"

[[ "$BRIDGE_CONFIRM_OPERATIONAL_CONFIG_SEAL" == SEAL_PRODUCTION_OPERATIONAL_CONFIG ]] || {
  echo "seal requires the exact explicit confirmation token" >&2
  exit 1
}

[[ -f "$BRIDGE_PRODUCTION_CONTROLLER_PEM" && ! -L "$BRIDGE_PRODUCTION_CONTROLLER_PEM" ]] || {
  echo "production controller identity PEM must be an ordinary file" >&2
  exit 1
}

FROZEN_BUNDLE="$(mktemp -d "${TMPDIR:-/tmp}/bridge-seal-plan.XXXXXX")"
trap 'chmod u+w "$FROZEN_BUNDLE" 2>/dev/null || true; rm -rf "$FROZEN_BUNDLE"' EXIT
production_freeze_bundle "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_BUNDLE"
BRIDGE_RELEASE_BUNDLE="$FROZEN_BUNDLE"

PARAMETERS="$BRIDGE_RELEASE_BUNDLE/initial-operational-parameters.json"
[[ -f "$PARAMETERS" && ! -L "$PARAMETERS" ]] || {
  echo "Gate B initial operational parameters are missing" >&2
  exit 1
}

production_validate_gate \
  gate-b-pre-seal "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256"

read -r BRIDGE_CANISTER_ID IC_HOST < <(
  python3 -c '
import json,sys
p=json.load(open(sys.argv[1],encoding="utf-8"))
print(p.get("bridge_canister_id",""),p.get("ic_host",""))
' "$BRIDGE_RELEASE_BUNDLE/profile.json"
)
[[ -n "$BRIDGE_CANISTER_ID" && -n "$IC_HOST" ]] || {
  echo "Gate B profile is missing the Bridge Canister or IC host" >&2
  exit 1
}
export BRIDGE_CANISTER_ID IC_HOST
export IC_IDENTITY_PEM="$BRIDGE_PRODUCTION_CONTROLLER_PEM"

node --no-warnings --experimental-strip-types \
  "$SOURCE_ROOT/tools/governance-relayer/cli.ts" seal-operational-config \
  --parameters-file "$PARAMETERS" \
  --receipt-file "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT"
