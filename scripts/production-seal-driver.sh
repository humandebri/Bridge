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
PROFILE=(cargo run --locked --quiet --release --manifest-path "$SOURCE_ROOT/Cargo.toml" -p bridge-profile --)
RESERVATION="$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT.reservation.json"
ATTEMPT="$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT.attempt.json"
[[ "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" == /* \
  && ! -e "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
  && ! -L "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
  && -d "$(dirname "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT")" ]] || {
  echo "seal receipt must be a new absolute file in an existing directory" >&2
  exit 1
}
for sidecar in "$RESERVATION" "$ATTEMPT"; do
  [[ ! -L "$sidecar" ]] || { echo "seal sidecar must not be a symlink: $sidecar" >&2; exit 1; }
done
if [[ ! -e "$RESERVATION" && -e "$ATTEMPT" ]]; then
  echo "seal attempt exists without its durable reservation; no update was sent" >&2
  exit 1
fi

RESERVATION_STATE="$("${PROFILE[@]}" reserve-operational-config-seal \
  "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" "$RESERVATION")"
[[ "$RESERVATION_STATE" == created || "$RESERVATION_STATE" == existing ]] || {
  echo "unexpected operational config seal reservation result" >&2
  exit 1
}

if [[ "$RESERVATION_STATE" == created ]]; then
  set +e
  node --no-warnings --experimental-strip-types \
    "$SOURCE_ROOT/tools/governance-relayer/cli.ts" seal-operational-config \
    --parameters-file "$PARAMETERS" \
    --receipt-file "$ATTEMPT"
  SEAL_STATUS=$?
  set -e
else
  SEAL_STATUS=0
fi

ATTEMPT_ARG=-
if [[ -f "$ATTEMPT" ]]; then ATTEMPT_ARG="$ATTEMPT"; fi
if ! "${PROFILE[@]}" write-operational-config-seal-receipt \
  "$BRIDGE_RELEASE_BUNDLE" "$RESERVATION" "$ATTEMPT_ARG" \
  "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT"; then
  if [[ "$RESERVATION_STATE" == existing && ! -f "$ATTEMPT" ]]; then
    echo "seal reservation exists without a proven live seal; no update was resent" >&2
  elif [[ "$SEAL_STATUS" -ne 0 ]]; then
    echo "seal response was ambiguous and the live postconditions did not prove success" >&2
  fi
  exit 1
fi

echo "production operational config seal verified: $BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT"
