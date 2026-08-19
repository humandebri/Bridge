#!/usr/bin/env bash
# Create the production Bridge Canister once on the pinned fiduciary subnet.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CANISTER="bridge-canister"
ENVIRONMENT="production"
SUBNET="pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae"
REGISTRY="rwlgt-iiaaa-aaaaa-aaaaa-cai"
MAPPING="$ROOT/.icp/data/mappings/production.ids.json"

: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP CLI identity}"
command -v icp >/dev/null || { echo "icp is required" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }
[[ "$(icp --version)" == "icp 1.0.2" ]] || {
  echo "production bootstrap requires icp 1.0.2" >&2
  exit 1
}

mapping_id() {
  python3 - "$MAPPING" "$CANISTER" <<'PY'
import json, os, sys
path, name = sys.argv[1:]
if not os.path.exists(path):
    print("")
    raise SystemExit
value = json.load(open(path, encoding="utf-8"))
if not isinstance(value, dict):
    raise SystemExit("production mapping must be a JSON object")
canister_id = value.get(name, "")
if not isinstance(canister_id, str):
    raise SystemExit("production Bridge Canister mapping must be a string")
print(canister_id)
PY
}

CANISTER_ID="$(mapping_id)"
if [[ -z "$CANISTER_ID" ]]; then
  icp canister create "$CANISTER" \
    -e "$ENVIRONMENT" \
    --subnet "$SUBNET" \
    --identity "$BRIDGE_ICP_IDENTITY" \
    --project-root-override "$ROOT"
  CANISTER_ID="$(mapping_id)"
  [[ -n "$CANISTER_ID" ]] || {
    echo "production Canister creation did not update the mapping" >&2
    exit 1
  }
else
  echo "production mapping already contains $CANISTER=$CANISTER_ID; skipping creation"
fi

STATUS_ID="$(icp canister status "$CANISTER" -e "$ENVIRONMENT" -i \
  --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT")"
[[ "$STATUS_ID" == "$CANISTER_ID" ]] || {
  echo "production mapping does not match the reachable Bridge Canister" >&2
  exit 1
}

SUBNET_RESPONSE="$(icp canister call "$REGISTRY" get_subnet_for_canister \
  "(record { \"principal\" = opt principal \"$CANISTER_ID\" })" --query -n ic \
  --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT")"
ACTUAL_SUBNET="$(python3 - "$SUBNET_RESPONSE" <<'PY'
import re, sys
values = re.findall(r'principal\s+"([a-z0-9-]+)"', sys.argv[1])
if len(values) != 1:
    raise SystemExit("NNS Registry did not return exactly one subnet principal")
print(values[0])
PY
)"
[[ "$ACTUAL_SUBNET" == "$SUBNET" ]] || {
  echo "production Bridge Canister is on unexpected subnet: $ACTUAL_SUBNET" >&2
  exit 1
}

echo "production Bridge Canister verified: id=$CANISTER_ID subnet=$ACTUAL_SUBNET"
