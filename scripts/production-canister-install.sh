#!/usr/bin/env bash
# Install and initialize the fixed production Bridge Canister exactly once.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source "$ROOT/scripts/production-validation.sh"

PLAN=""
WASM=""
RECEIPT=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --plan) [[ "$#" -ge 2 ]] || exit 2; PLAN="$2"; shift 2 ;;
    --wasm) [[ "$#" -ge 2 ]] || exit 2; WASM="$2"; shift 2 ;;
    --receipt) [[ "$#" -ge 2 ]] || exit 2; RECEIPT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP CLI identity}"
RESERVATION="$RECEIPT.reservation"
[[ -f "$PLAN" && -f "$WASM" && -n "$RECEIPT" && ! -e "$RECEIPT" && ! -e "$RESERVATION" ]] || {
  echo "usage: BRIDGE_ICP_IDENTITY=NAME $0 --plan PLAN.json --wasm bridge-canister.wasm --receipt RECEIPT.json" >&2
  exit 2
}
for tool in cargo icp python3 shasum; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done
[[ "$(icp --version)" == "icp 1.0.2" ]] || {
  echo "production Canister install requires icp 1.0.2" >&2
  exit 1
}
production_require_clean_source "$ROOT"

TARGET="$(mktemp -d "${TMPDIR:-/tmp}/bridge-profile-install.XXXXXX")"
INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-canister-inputs.XXXXXX")"
RECEIPT_TMP="$RECEIPT.tmp.$$"
trap 'rm -rf "$TARGET" "$INPUTS"; rm -f "$RECEIPT_TMP"' EXIT
python3 - "$PLAN" "$WASM" "$INPUTS/production-canister-plan.json" "$INPUTS/bridge-canister.wasm" <<'PY'
import os, stat, sys
for source, target, limit in ((sys.argv[1], sys.argv[3], 1024 * 1024), (sys.argv[2], sys.argv[4], 100 * 1024 * 1024)):
    fd = os.open(source, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_size <= 0 or info.st_size > limit:
            raise SystemExit("production Canister input is not a bounded regular file")
        chunks = []
        while True:
            chunk = os.read(fd, 1024 * 1024)
            if not chunk: break
            chunks.append(chunk)
    finally:
        os.close(fd)
    out = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o400)
    try:
        for chunk in chunks: os.write(out, chunk)
        os.fsync(out)
    finally:
        os.close(out)
PY
PLAN="$INPUTS/production-canister-plan.json"
WASM="$INPUTS/bridge-canister.wasm"
CARGO_TARGET_DIR="$TARGET" cargo build --locked --quiet --release \
  --manifest-path "$ROOT/Cargo.toml" -p bridge-profile
PROFILE_BIN="$TARGET/release/bridge-profile"
[[ -x "$PROFILE_BIN" ]] || { echo "bridge-profile build did not produce an executable" >&2; exit 1; }
"$PROFILE_BIN" validate-production-canister-plan "$PLAN" >/dev/null
"$PROFILE_BIN" render-production-canister-inputs "$PLAN" "$INPUTS" >/dev/null

CURRENT_REVISION="$(git -C "$ROOT" rev-parse HEAD)"
CURRENT_TREE="$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print $1}')"
WASM_SHA256="$(shasum -a 256 "$WASM" | awk '{print tolower($1)}')"
read -r PLAN_REVISION PLAN_TREE CANISTER_ID PLAN_WASM < <(python3 - "$PLAN" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
print(value["source_revision"], value["source_tree_sha256"].lower(), value["bridge_canister_id"], value["bridge_canister_wasm_sha256"].lower())
PY
)
[[ "$PLAN_REVISION" == "$CURRENT_REVISION" && "$PLAN_TREE" == "$CURRENT_TREE" \
  && "$PLAN_WASM" == "$WASM_SHA256" ]] || {
  echo "production Canister plan/Wasm is not bound to the current clean source" >&2
  exit 1
}
MAPPED_ID="$(python3 - "$ROOT/.icp/data/mappings/production.ids.json" <<'PY'
import json, sys
value = json.load(open(sys.argv[1], encoding="utf-8"))
if not isinstance(value, dict) or not isinstance(value.get("bridge-canister"), str):
    raise SystemExit("production mapping does not contain bridge-canister")
print(value["bridge-canister"])
PY
)"
[[ "$MAPPED_ID" == "$CANISTER_ID" ]] || {
  echo "production mapping differs from the reviewed Canister plan" >&2
  exit 1
}
INSTALLER="$(icp identity principal --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT")"

status_fields() {
  python3 - "$1" <<'PY'
import json, re, sys
value = json.loads(sys.argv[1])
def named(node, name):
    out=[]
    if isinstance(node, dict):
        for key, child in node.items():
            if key == name: out.append(child)
            out += named(child, name)
    elif isinstance(node, list):
        for child in node: out += named(child, name)
    return out
def bytes_value(node):
    if node is None: return None
    if isinstance(node, str):
        text=node.removeprefix("0x")
        if re.fullmatch(r"[0-9a-fA-F]{64}", text): return text.lower()
    if isinstance(node, list) and len(node)==32 and all(isinstance(v,int) and not isinstance(v,bool) and 0<=v<=255 for v in node):
        return bytes(node).hex()
    if isinstance(node, dict) and len(node)==1:
        return bytes_value(next(iter(node.values())))
    return None
controllers=named(value,"controllers")
modules=named(value,"module_hash")
if len(controllers)!=1 or not isinstance(controllers[0],list): raise SystemExit("ambiguous Canister controller status")
module=None
if len(modules)>1: raise SystemExit("ambiguous Canister module hash status")
if modules: module=bytes_value(modules[0])
print(",".join(str(v) for v in controllers[0]), module or "-")
PY
}

PRE_STATUS="$(icp canister status "$CANISTER_ID" -n ic --json --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT")"
read -r PRE_CONTROLLERS PRE_MODULE < <(status_fields "$PRE_STATUS")
[[ "$PRE_CONTROLLERS" == "$INSTALLER" && "$PRE_MODULE" == "-" ]] || {
  echo "production Canister must be empty and controlled only by the installer" >&2
  exit 1
}

production_require_clean_source "$ROOT"
[[ "$(git -C "$ROOT" rev-parse HEAD)" == "$CURRENT_REVISION" \
  && "$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print $1}')" == "$CURRENT_TREE" ]] || {
  echo "source changed after the production Canister inputs were frozen" >&2
  exit 1
}
production_reserve_output "$RESERVATION" "production Canister install reservation"
icp canister install "$CANISTER_ID" -n ic --mode install --wasm "$WASM" \
  --args-file "$INPUTS/canister-init.bin" --args-format bin --yes \
  --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT" || {
  echo "Canister install result is unknown; inspect live status and do not rerun" >&2
  exit 1
}

call_hex() {
  local method="$1" args="$2" query="${3:-false}"
  if [[ "$query" == true ]]; then
    icp canister call "$CANISTER_ID" "$method" "$args" -n ic --query \
      --identity "$BRIDGE_ICP_IDENTITY" --candid "$ROOT/canister/bridge-canister/bridge.did" \
      --output hex --project-root-override "$ROOT"
  else
    icp canister call "$CANISTER_ID" "$method" "$args" -n ic \
      --identity "$BRIDGE_ICP_IDENTITY" --candid "$ROOT/canister/bridge-canister/bridge.did" \
      --output hex --project-root-override "$ROOT"
  fi
}

INIT_RESPONSE="$(call_hex initialize_public_config '()')"
VALIDATION_RESPONSE="$(call_hex start_storage_validation '()')"
for _ in {1..1024}; do
  [[ "$("$PROFILE_BIN" storage-validation-complete "$VALIDATION_RESPONSE")" == true ]] && break
  VALIDATION_RESPONSE="$(call_hex continue_storage_validation '(1000 : nat16)')"
done
[[ "$("$PROFILE_BIN" storage-validation-complete "$VALIDATION_RESPONSE")" == true ]] || {
  echo "storage validation did not complete within the fixed bound" >&2; exit 1;
}
CHECKSUM_RESPONSE="$(call_hex refresh_storage_checksum '(10485760 : nat64)')"
for _ in {1..1024}; do
  [[ "$("$PROFILE_BIN" storage-checksum-complete "$CHECKSUM_RESPONSE")" == true ]] && break
  CHECKSUM_RESPONSE="$(call_hex refresh_storage_checksum '(10485760 : nat64)')"
done
[[ "$("$PROFILE_BIN" storage-checksum-complete "$CHECKSUM_RESPONSE")" == true ]] || {
  echo "storage checksum did not complete within the fixed bound" >&2; exit 1;
}

RUNTIME_RESPONSE="$(call_hex get_runtime_binding '()' true)"
OPERATIONAL_RESPONSE="$(call_hex get_operational_config '()' true)"
STATUS_RESPONSE="$(call_hex get_bridge_status '()' true)"
LIFECYCLE_RESPONSE="$(call_hex get_production_lifecycle '()' true)"
INTEGRITY_RESPONSE="$(call_hex storage_integrity_check '()' true)"
POST_STATUS="$(icp canister status "$CANISTER_ID" -n ic --json --identity "$BRIDGE_ICP_IDENTITY" --project-root-override "$ROOT")"
read -r POST_CONTROLLERS POST_MODULE < <(status_fields "$POST_STATUS")
[[ "$POST_CONTROLLERS" == "$INSTALLER" && "$POST_MODULE" == "$WASM_SHA256" ]] || {
  echo "installed module or controller set differs from the approved plan" >&2
  exit 1
}

"$PROFILE_BIN" write-production-canister-receipt "$PLAN" "$INSTALLER" "$POST_MODULE" \
  "$INIT_RESPONSE" "$VALIDATION_RESPONSE" "$CHECKSUM_RESPONSE" "$RUNTIME_RESPONSE" \
  "$OPERATIONAL_RESPONSE" "$STATUS_RESPONSE" "$LIFECYCLE_RESPONSE" "$INTEGRITY_RESPONSE" \
  "$RECEIPT_TMP"
production_atomic_replace "$RECEIPT_TMP" "$RECEIPT"
rm "$RESERVATION"
python3 - "$RECEIPT" <<'PY'
import os, sys
parent = os.path.dirname(os.path.abspath(sys.argv[1])) or "."
fd = os.open(parent, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
try: os.fsync(fd)
finally: os.close(fd)
PY
echo "Production Canister installed paused; receipt=$RECEIPT"
