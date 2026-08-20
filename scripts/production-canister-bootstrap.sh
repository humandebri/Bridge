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

STATUS_JSON="$(icp canister status "$CANISTER_ID" -n ic --json \
  --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT")"
EXECUTING_PRINCIPAL="$(icp identity principal --identity "$BRIDGE_ICP_IDENTITY" \
  --project-root-override "$ROOT")"
python3 - "$STATUS_JSON" "$EXECUTING_PRINCIPAL" <<'PY'
import json, sys

status = json.loads(sys.argv[1])
caller = sys.argv[2]

def values(value, key):
    found = []
    if isinstance(value, dict):
        for name, child in value.items():
            if name == key:
                found.append(child)
            found.extend(values(child, key))
    elif isinstance(value, list):
        for child in value:
            found.extend(values(child, key))
    return found

controller_values = values(status, "controllers")
if len(controller_values) != 1 or not isinstance(controller_values[0], list):
    raise SystemExit("production Canister status does not expose one controller set")
controllers = [str(value) for value in controller_values[0]]
if controllers != [caller]:
    raise SystemExit("bootstrap Canister controllers must be exactly the reviewed production identity")
module_hash_values = values(status, "module_hash")
if len(module_hash_values) > 1:
    raise SystemExit("production Canister status exposes ambiguous module_hash values")
if module_hash_values:
    module_hash = module_hash_values[0]
    if module_hash not in (None, [], "", {"None": None}):
        raise SystemExit("bootstrap Canister already has a Wasm module installed")
PY

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
