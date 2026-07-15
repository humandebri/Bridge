#!/usr/bin/env bash
# Production release entrypoint: require evidence gates before deployment or asset activation.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODE="${1:-}"
shift || true

BUNDLE=""
CONFIRMATION=""
RECEIPT=""
RELEASE_INPUTS=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --bundle)
      [[ "$#" -ge 2 ]] || { echo "--bundle requires a directory" >&2; exit 2; }
      BUNDLE="$2"
      shift 2
      ;;
    --receipt)
      [[ "$#" -ge 2 ]] || { echo "--receipt requires a path" >&2; exit 2; }
      RECEIPT="$2"
      shift 2
      ;;
    --release-inputs)
      [[ "$#" -ge 2 ]] || { echo "--release-inputs requires a directory" >&2; exit 2; }
      RELEASE_INPUTS="$2"
      shift 2
      ;;
    --confirm-asset-acceptance)
      [[ "$#" -ge 2 ]] || { echo "--confirm-asset-acceptance requires a value" >&2; exit 2; }
      CONFIRMATION="$2"
      shift 2
      ;;
    --)
      shift
      break
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

usage() {
  echo "usage: $0 deploy --bundle DIR --release-inputs DIR --receipt FILE -- DEPLOY_DRIVER" >&2
  echo "       $0 activate --bundle DIR --release-inputs DIR --receipt FILE --confirm-asset-acceptance UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE -- scripts/production-activate-driver.sh" >&2
  exit 2
}

[[ "$MODE" == "deploy" || "$MODE" == "activate" ]] || usage
[[ -n "$BUNDLE" && -d "$BUNDLE" ]] || { echo "a readable Gate bundle directory is required" >&2; exit 1; }
[[ -n "$RELEASE_INPUTS" && -d "$RELEASE_INPUTS" ]] || { echo "reviewed release inputs are required" >&2; exit 1; }
[[ -n "$RECEIPT" ]] || { echo "a Gate A receipt path is required" >&2; exit 1; }
[[ "$#" -eq 1 && -x "$1" ]] || { echo "exactly one executable release driver is required; arbitrary deployment arguments are forbidden" >&2; exit 2; }

SOURCE_ROOT="$ROOT"
SOURCE_ROOT="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$SOURCE_ROOT")"
[[ -d "$SOURCE_ROOT/.git" ]] || { echo "production source root is not a Git worktree" >&2; exit 1; }
git -C "$SOURCE_ROOT" diff --quiet --exit-code
git -C "$SOURCE_ROOT" diff --cached --quiet --exit-code
[[ -z "$(git -C "$SOURCE_ROOT" ls-files --others --exclude-standard)" ]] || {
  echo "production release rejects an untracked source tree" >&2
  exit 1
}
DRIVER_PATH="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1")"
DRIVER_RELATIVE="$(python3 -c 'import os,sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$DRIVER_PATH" "$SOURCE_ROOT")"
EXPECTED_DRIVER="production-${MODE}-driver.sh"
[[ "$DRIVER_PATH" == "$SOURCE_ROOT/scripts/$EXPECTED_DRIVER" ]] || {
  echo "release driver must be the reviewed $EXPECTED_DRIVER in the bound source tree" >&2
  exit 1
}
git -C "$SOURCE_ROOT" ls-files --error-unmatch "$DRIVER_RELATIVE" >/dev/null || {
  echo "release driver is not tracked by the bound source revision" >&2
  exit 1
}
CURRENT_SOURCE_REVISION="$(git -C "$SOURCE_ROOT" rev-parse HEAD)"
CURRENT_SOURCE_TREE_SHA256="$(git -C "$SOURCE_ROOT" archive HEAD | shasum -a 256 | awk '{print $1}')"
read -r RELEASE_ID MANIFEST_SOURCE_REVISION MANIFEST_SOURCE_TREE_SHA256 PROFILE_SHA256 CANISTER_WASM_SHA256 BRIDGE_RUNTIME_SHA256 PARENT_GATE_A_MANIFEST_SHA256 < <(
  python3 -c '
import json, sys
manifest = json.load(open(sys.argv[1], encoding="utf-8"))
profile = next((item["sha256"] for item in manifest["artifacts"] if item["path"] == "profile.json"), "")
wasm = next((item["sha256"] for item in manifest["artifacts"] if item["path"] == "bridge-canister.wasm"), "")
runtime = next((item["sha256"] for item in manifest["artifacts"] if item["path"] == "bridge-runtime.bin"), "")
print(manifest.get("release_id", ""), manifest.get("source_revision", ""), manifest.get("source_tree_sha256", ""), profile, wasm, runtime, manifest.get("parent_gate_a_manifest_sha256", "-"))
' "$BUNDLE/release-manifest.json"
)
[[ "$MANIFEST_SOURCE_REVISION" == "$CURRENT_SOURCE_REVISION" \
  && "$(printf '%s' "$MANIFEST_SOURCE_TREE_SHA256" | tr '[:upper:]' '[:lower:]')" == "$CURRENT_SOURCE_TREE_SHA256" \
  && "$PROFILE_SHA256" =~ ^[0-9a-fA-F]{64}$ \
  && "$CANISTER_WASM_SHA256" =~ ^[0-9a-fA-F]{64}$ \
  && "$BRIDGE_RUNTIME_SHA256" =~ ^[0-9a-fA-F]{64}$ ]] || {
  echo "release bundle does not bind the current clean source/profile" >&2
  exit 1
}

RENDERED_INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-release-inputs.XXXXXX")"
PROFILE_TARGET="$(mktemp -d "${TMPDIR:-/tmp}/bridge-profile-build.XXXXXX")"
trap 'rm -rf "$RENDERED_INPUTS" "$PROFILE_TARGET"' EXIT
CARGO_TARGET_DIR="$PROFILE_TARGET" cargo build --locked --quiet --release \
  --manifest-path "$SOURCE_ROOT/Cargo.toml" -p bridge-profile
PROFILE_BIN="$PROFILE_TARGET/release/bridge-profile"
[[ -x "$PROFILE_BIN" ]] || { echo "reviewed bridge-profile build did not produce an executable" >&2; exit 1; }
run_profile_gate() { "$PROFILE_BIN" "$@"; }
if [[ "$MODE" == "activate" ]]; then
  run_profile_gate render-bundle-inputs "$BUNDLE" "$RENDERED_INPUTS" >/dev/null
else
  run_profile_gate render-release-inputs "$BUNDLE/profile.json" "$RENDERED_INPUTS" >/dev/null
fi
if ! diff -qr "$RENDERED_INPUTS" "$RELEASE_INPUTS" >/dev/null; then
  echo "release inputs drift from the approved profile" >&2
  exit 1
fi
INPUT_PROFILE_SHA256="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["profile_file_sha256"])' "$RELEASE_INPUTS/release-inputs-manifest.json")"
[[ "$(printf '%s' "$INPUT_PROFILE_SHA256" | tr '[:upper:]' '[:lower:]')" \
  == "$(printf '%s' "$PROFILE_SHA256" | tr '[:upper:]' '[:lower:]')" ]] || {
  echo "release inputs are not bound to the bundle profile" >&2
  exit 1
}
export BRIDGE_CANISTER_INIT_FILE="$RELEASE_INPUTS/canister-init.json"
export BRIDGE_CONSTRUCTOR_ARGS_FILE="$RELEASE_INPUTS/contract-constructor-args.json"
export BRIDGE_UI_RUNTIME_PROFILE_FILE="$RELEASE_INPUTS/ui-runtime-profile.json"
export BRIDGE_RELEASE_INPUTS_MANIFEST="$RELEASE_INPUTS/release-inputs-manifest.json"
export BRIDGE_RELEASE_BUNDLE="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$BUNDLE")"
export BRIDGE_SOURCE_ROOT="$SOURCE_ROOT"

GATE_OUTPUT=""
if [[ "$MODE" == "deploy" ]]; then
  GATE_OUTPUT="$(run_profile_gate validate-bundle --offline "$BUNDLE")"
  printf '%s\n' "$GATE_OUTPUT"
else
  [[ "$CONFIRMATION" == "UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE" ]] || {
    echo "Gate B activation requires explicit asset-acceptance confirmation" >&2
    exit 1
  }
  [[ -f "$RECEIPT" ]] || { echo "Gate B requires the matching Gate A receipt" >&2; exit 1; }
  [[ -f "$BUNDLE/gate-a-receipt.json" ]] || { echo "Gate B bundle is missing its Gate A receipt artifact" >&2; exit 1; }
  cmp -s "$RECEIPT" "$BUNDLE/gate-a-receipt.json" || {
    echo "external Gate A receipt differs from the Gate B receipt artifact" >&2
    exit 1
  }
  RECEIPT_MANIFEST_SHA256="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["gate_a_manifest_sha256"])' "$RECEIPT")"
  [[ "$PARENT_GATE_A_MANIFEST_SHA256" == "$RECEIPT_MANIFEST_SHA256" ]] || {
    echo "Gate B manifest is not descended from the Gate A receipt" >&2
    exit 1
  }
  python3 -c '
import json, sys
r = json.load(open(sys.argv[1], encoding="utf-8"))
expected = sys.argv[2:9]
actual = [r.get("gate_a_manifest_sha256"), r.get("release_id"), r.get("source_revision"), r.get("source_tree_sha256"), r.get("profile_sha256"), r.get("bridge_canister_wasm_sha256"), r.get("bridge_runtime_bytecode_sha256")]
raise SystemExit(0 if [str(v).lower() for v in actual] == [v.lower() for v in expected] else 1)
' "$RECEIPT" "$RECEIPT_MANIFEST_SHA256" "$RELEASE_ID" \
    "$CURRENT_SOURCE_REVISION" "$CURRENT_SOURCE_TREE_SHA256" "$PROFILE_SHA256" "$CANISTER_WASM_SHA256" "$BRIDGE_RUNTIME_SHA256" || {
    echo "Gate A receipt does not match the current release" >&2
    exit 1
  }
  LIVE_PREFLIGHT_PATH="$SOURCE_ROOT/scripts/production-live-preflight.sh"
  [[ -x "$LIVE_PREFLIGHT_PATH" ]] || {
    echo "Gate B requires the reviewed live snapshot preflight" >&2
    exit 1
  }
  LIVE_PREFLIGHT_RELATIVE="scripts/production-live-preflight.sh"
  git -C "$SOURCE_ROOT" ls-files --error-unmatch "$LIVE_PREFLIGHT_RELATIVE" >/dev/null \
    || { echo "live preflight is not tracked by the bound source revision" >&2; exit 1; }
  "$LIVE_PREFLIGHT_PATH" verify "$BUNDLE"
  GATE_OUTPUT="$(run_profile_gate verify-live "$BUNDLE")"
  printf '%s\n' "$GATE_OUTPUT"
fi

[[ "$GATE_OUTPUT" =~ manifest_sha256=([0-9a-fA-F]{64}) ]] || {
  echo "evidence gate did not return a manifest hash" >&2
  exit 1
}
GATE_MANIFEST_SHA256="${BASH_REMATCH[1]}"
if [[ "$MODE" == "deploy" ]]; then
  export BRIDGE_GATE_A_MANIFEST_SHA256="$GATE_MANIFEST_SHA256"
  "$DRIVER_PATH"
  RECEIPT_TMP="$RECEIPT.tmp.$$"
  python3 -c '
import json, sys
with open(sys.argv[1], "w", encoding="utf-8") as output:
    json.dump({"gate_a_manifest_sha256": sys.argv[2], "release_id": sys.argv[3], "source_revision": sys.argv[4], "source_tree_sha256": sys.argv[5], "profile_sha256": sys.argv[6], "bridge_canister_wasm_sha256": sys.argv[7], "bridge_runtime_bytecode_sha256": sys.argv[8]}, output, sort_keys=True, separators=(",", ":"))
    output.write("\n")
' "$RECEIPT_TMP" "$GATE_MANIFEST_SHA256" "$RELEASE_ID" "$CURRENT_SOURCE_REVISION" "$CURRENT_SOURCE_TREE_SHA256" "$PROFILE_SHA256" "$CANISTER_WASM_SHA256" "$BRIDGE_RUNTIME_SHA256"
  mv "$RECEIPT_TMP" "$RECEIPT"
else
  export BRIDGE_GATE_B_MANIFEST_SHA256="$GATE_MANIFEST_SHA256"
  exec "$DRIVER_PATH"
fi
