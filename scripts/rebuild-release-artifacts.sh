#!/usr/bin/env bash
# Rebuild release artifacts twice from the fixed clean source and compare hashes.
set -euo pipefail

SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUNDLE="${1:?missing release bundle}"
EXPECTED_REVISION="${2:?missing source revision}"
EXPECTED_TREE="${3:?missing source tree hash}"
FIRST="$(mktemp -d "${TMPDIR:-/tmp}/bridge-rebuild-first.XXXXXX")"
SECOND="$(mktemp -d "${TMPDIR:-/tmp}/bridge-rebuild-second.XXXXXX")"
trap 'rm -rf "$FIRST" "$SECOND"' EXIT
EXPECTED_TREE="$(printf '%s' "$EXPECTED_TREE" | tr '[:upper:]' '[:lower:]')"
EXPECTED_SUBMODULES="$(git -C "$SOURCE_ROOT" submodule status --recursive)"

[[ -f "$BUNDLE/release-manifest.json" ]] || { echo "release manifest is missing" >&2; exit 1; }

verify_source() {
  [[ -z "$(git -C "$SOURCE_ROOT" status --porcelain=v1 --untracked-files=all --ignore-submodules=none)" ]] \
    || { echo "reproducible build requires a clean source tree" >&2; return 1; }
  [[ "$(git -C "$SOURCE_ROOT" rev-parse HEAD)" == "$EXPECTED_REVISION" ]] \
    || { echo "reproducible build revision mismatch" >&2; return 1; }
  [[ "$(git -C "$SOURCE_ROOT" archive HEAD | shasum -a 256 | awk '{print $1}')" == "$EXPECTED_TREE" ]] \
    || { echo "reproducible build tree mismatch" >&2; return 1; }
  [[ "$(git -C "$SOURCE_ROOT" submodule status --recursive)" == "$EXPECTED_SUBMODULES" ]] \
    || { echo "reproducible build submodule mismatch" >&2; return 1; }
}

verify_source

build_once() {
  local output="$1"
  mkdir -p "$output/cargo" "$output/forge"
  verify_source
  CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$output/cargo" \
    icp build bridge-canister -e production --project-root-override "$SOURCE_ROOT"
  verify_source
  cp "$output/cargo/wasm32-unknown-unknown/release/bridge_canister.wasm" \
    "$output/bridge-canister.wasm"
  verify_source
  FOUNDRY_PROFILE=default FOUNDRY_OFFLINE=true forge build --offline --force --root "$SOURCE_ROOT/contracts" \
    --out "$output/forge" --cache-path "$output/forge-cache"
  verify_source
  python3 - \
    "$output/forge/BSNS.sol/BSNS.json" "$output/bsns-creation.bin" "$output/bsns-runtime.bin" "$output/bsns-runtime-layout.json" <<'PY'
import json, pathlib, sys
bsns_source, bsns_creation_target, bsns_runtime_target, bsns_layout_target = sys.argv[1:]
def write_bytecode(source, field, target, label):
    value = json.load(open(source, encoding="utf-8"))[field]["object"]
    if not isinstance(value, str) or not value.startswith("0x") or not value[2:]:
        raise SystemExit(f"{label} bytecode is missing")
    pathlib.Path(target).write_bytes(bytes.fromhex(value[2:]))
write_bytecode(bsns_source, "bytecode", bsns_creation_target, "BSNS creation")
artifact=json.load(open(bsns_source, encoding="utf-8"))
runtime=bytearray.fromhex(artifact["deployedBytecode"]["object"].removeprefix("0x"))
ranges=[]
for values in artifact["deployedBytecode"].get("immutableReferences", {}).values():
    for value in values:
        start=value.get("start"); length=value.get("length")
        if not isinstance(start,int) or not isinstance(length,int) or start < 0 or length <= 0 or start + length > len(runtime):
            raise SystemExit("BSNS immutable reference layout is invalid")
        ranges.append({"start":start,"length":length})
if not ranges: raise SystemExit("BSNS runtime has no immutable reference layout")
for value in ranges: runtime[value["start"]:value["start"]+value["length"]]=bytes(value["length"])
pathlib.Path(bsns_runtime_target).write_bytes(runtime)
pathlib.Path(bsns_layout_target).write_text(json.dumps({"schema_version":1,"byte_length":len(runtime),"immutable_ranges":sorted(ranges,key=lambda x:(x["start"],x["length"]))},sort_keys=True,separators=(",",":"))+"\n")
PY
  bash "$SOURCE_ROOT/scripts/concretize-bridge-runtime.sh" \
    "$SOURCE_ROOT" "$BUNDLE/profile.json" "$output/bridge-runtime.bin"
  verify_source
}

build_once "$FIRST"
build_once "$SECOND"
verify_source
python3 "$SOURCE_ROOT/scripts/check_reproducible_artifacts.py" "$BUNDLE" "$FIRST" "$SECOND"
verify_source
