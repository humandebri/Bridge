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
  python3 - "$output/forge/Bridge.sol/Bridge.json" "$output/bridge-runtime.bin" <<'PY'
import json, pathlib, sys
source, target = sys.argv[1:]
value = json.load(open(source, encoding="utf-8"))["deployedBytecode"]["object"]
if not isinstance(value, str) or not value.startswith("0x"):
    raise SystemExit("Bridge runtime bytecode is missing")
pathlib.Path(target).write_bytes(bytes.fromhex(value[2:]))
PY
  verify_source
}

build_once "$FIRST"
build_once "$SECOND"
verify_source
python3 "$SOURCE_ROOT/scripts/check_reproducible_artifacts.py" "$BUNDLE" "$FIRST" "$SECOND"
verify_source
