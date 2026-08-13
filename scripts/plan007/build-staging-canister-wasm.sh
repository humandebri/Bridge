#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RAW_WASM="$ROOT/target/test-deployment/wasm32-unknown-unknown/release/bridge_canister.wasm"
DEFAULT_OUTPUT="$ROOT/target/test-deployment/staging/bridge_canister.wasm"
OUTPUT="${1:-$DEFAULT_OUTPUT}"
DID="$ROOT/canister/bridge-canister/bridge.did"

for tool in cargo ic-wasm; do
  command -v "$tool" >/dev/null 2>&1 || { echo >&2 "$tool is required"; exit 1; }
done

if [[ "$OUTPUT" != /* ]]; then
  OUTPUT="$ROOT/$OUTPUT"
fi
mkdir -p "$(dirname "$OUTPUT")"

CARGO_TARGET_DIR="$ROOT/target/test-deployment" cargo build \
  --locked \
  --manifest-path "$ROOT/Cargo.toml" \
  --target wasm32-unknown-unknown \
  --release \
  -p bridge-canister \
  --features test-deployment

temporary_wasm="$(mktemp "$(dirname "$OUTPUT")/.bridge-canister.wasm.XXXXXX")"
cleanup() {
  rm -f "$temporary_wasm"
}
trap cleanup EXIT

cp "$RAW_WASM" "$temporary_wasm"
ic-wasm "$temporary_wasm" -o "$temporary_wasm" \
  metadata "candid:service" -f "$DID" -v public --keep-name-section
ic-wasm "$temporary_wasm" -o "$temporary_wasm" \
  metadata "kinic:deployment" -d "test-deployment" --keep-name-section
ic-wasm "$temporary_wasm" -o "$temporary_wasm" shrink --keep-name-section

metadata_sections="$(ic-wasm "$temporary_wasm" metadata)"
if [[ "$(rg -c '^icp:public candid:service$' <<<"$metadata_sections")" != 1 ]] \
  || [[ "$(rg -c '^icp:private kinic:deployment$' <<<"$metadata_sections")" != 1 ]]; then
  echo "staging Wasm does not contain the reviewed metadata sections" >&2
  exit 1
fi
if ! cmp -s <(ic-wasm "$temporary_wasm" metadata "candid:service") <(cat "$DID"; printf '\n'); then
  echo "staging Wasm Candid metadata does not match the checked-in interface" >&2
  exit 1
fi
if [[ "$(ic-wasm "$temporary_wasm" metadata "kinic:deployment")" != "test-deployment" ]]; then
  echo "staging Wasm deployment metadata is invalid" >&2
  exit 1
fi

mv "$temporary_wasm" "$OUTPUT"
printf '%s\n' "$OUTPUT"
