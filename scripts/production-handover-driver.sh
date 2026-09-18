#!/usr/bin/env bash
# Add KINIC SNS Root as a co-controller after current-state validation.
set -euo pipefail
[[ $# -eq 0 ]] || { echo "usage: $0" >&2; exit 2; }

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$ROOT_DIR/scripts/production-validation.sh"

: "${BRIDGE_RELEASE_BUNDLE:?missing reviewed release bundle}"
: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP identity}"
: "${BRIDGE_HANDOVER_CONFIRMATION:?missing handover confirmation}"
[[ "$BRIDGE_ICP_IDENTITY" == production ]] || {
  echo "controller handover requires BRIDGE_ICP_IDENTITY=production" >&2; exit 1;
}
[[ "$BRIDGE_HANDOVER_CONFIRMATION" == STAGE_KINIC_SNS_ROOT_CO_CONTROLLER ]] || {
  echo "controller handover requires the exact confirmation phrase" >&2; exit 1;
}
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"
[[ "$BRIDGE_RELEASE_BUNDLE" == /* && -f "$PROFILE" && ! -L "$PROFILE" ]] || {
  echo "handover requires an absolute reviewed release bundle" >&2; exit 1;
}
for tool in cargo git icp python3 shasum; do
  command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done
production_require_clean_source "$ROOT_DIR"
REVISION="$(git -C "$ROOT_DIR" rev-parse HEAD)"
TREE="$(git -C "$ROOT_DIR" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')"
require_source_identity() {
  production_require_clean_source "$ROOT_DIR" \
    && [[ "$(git -C "$ROOT_DIR" rev-parse HEAD)" == "$REVISION" ]] \
    && [[ "$(git -C "$ROOT_DIR" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')" == "$TREE" ]] || {
      echo "source changed during controller handover" >&2
      return 1
    }
}

read -r CANISTER CONTROLLER SNS_ROOT < <(python3 -I -S - "$PROFILE" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]))
print(p['bridge_canister_id'],p['pause_principal'],p['root_canister_id'])
PY
)
[[ "$CANISTER" == lb5i5-ziaaa-aaaar-qcgwq-cai \
  && "$CONTROLLER" == lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe \
  && "$SNS_ROOT" == 7jkta-eyaaa-aaaaq-aaarq-cai ]] || {
  echo "release profile differs from the fixed production handover domain" >&2; exit 1;
}
[[ "$(icp identity principal --identity production)" == "$CONTROLLER" ]] || {
  echo "production identity differs from the expected sole controller" >&2; exit 1;
}
[[ "$(icp canister status bridge-canister -e production -i --identity production)" == "$CANISTER" ]] || {
  echo "production environment maps a different Bridge Canister" >&2; exit 1;
}

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-current-handover.XXXXXX")"
cleanup() { chmod -R u+w "$TMP" 2>/dev/null || true; rm -rf "$TMP"; }
trap cleanup EXIT
CARGO_TARGET_DIR="$TMP/profile" cargo build --quiet --locked --release --manifest-path "$ROOT_DIR/Cargo.toml" -p bridge-profile
PROFILE_BIN="$TMP/profile/release/bridge-profile"
production_run_proof_gate "$ROOT_DIR" "$REVISION" "$TREE"
for index in 1 2; do
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$TMP/repro-$index" \
    icp build bridge-canister -e production --project-root-override "$ROOT_DIR" >/dev/null
done
WASM_ONE="$TMP/repro-1/wasm32-unknown-unknown/release/bridge_canister.wasm"
WASM_TWO="$TMP/repro-2/wasm32-unknown-unknown/release/bridge_canister.wasm"
MODULE_SHA256="$(shasum -a 256 "$WASM_ONE" | awk '{print tolower($1)}')"
[[ -f "$WASM_ONE" && -f "$WASM_TWO" \
  && "$(shasum -a 256 "$WASM_TWO" | awk '{print tolower($1)}')" == "$MODULE_SHA256" ]] || {
  echo "current production Wasm is not reproducible" >&2; exit 1;
}
require_source_identity
export BRIDGE_PRODUCTION_INSTALLER_IDENTITY=production
"$PROFILE_BIN" verify-production-current-state "$PROFILE" "$CONTROLLER" "$MODULE_SHA256" sole
require_source_identity

set +e
OUTPUT="$(icp canister settings update bridge-canister -e production \
  --add-controller "$SNS_ROOT" --force --identity production --debug 2>&1)"
STATUS=$?
set -e
printf '%s\n' "$OUTPUT"
if "$PROFILE_BIN" verify-production-current-state "$PROFILE" "$CONTROLLER" "$MODULE_SHA256" joint; then
  printf 'production_handover=co-controller-ready module_sha256=%s\n' "$MODULE_SHA256"
  exit 0
fi
if [[ "$STATUS" -ne 0 ]]; then
  echo "controller update outcome is unresolved; do not retry for 6 minutes, then rerun authenticated validation" >&2
else
  echo "controller update returned success but the exact joint-control postcondition is absent" >&2
fi
exit 1
