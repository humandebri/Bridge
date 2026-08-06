#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
V30_SOURCE_COMMIT="d2eddc9fce7d41d5f84e0c5fba0073ccbbdcdc5f"
OUTPUT_DIR="$ROOT/target/v30-upgrade-fixture"
OUTPUT_WASM="$OUTPUT_DIR/bridge_canister_v30.wasm"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/bridge-v30-upgrade.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

if ! git -C "$ROOT" cat-file -e "$V30_SOURCE_COMMIT^{commit}" 2>/dev/null; then
  printf 'required reviewed v30 source commit is missing: %s\n' "$V30_SOURCE_COMMIT" >&2
  printf 'fetch the full repository history before running the proof gate\n' >&2
  exit 1
fi
git -C "$ROOT" archive --format=tar --output="$WORK_DIR/source.tar" "$V30_SOURCE_COMMIT"
mkdir "$WORK_DIR/source"
tar -xf "$WORK_DIR/source.tar" -C "$WORK_DIR/source"

CARGO_TARGET_DIR="$OUTPUT_DIR/build" cargo build \
  --locked \
  --manifest-path "$WORK_DIR/source/Cargo.toml" \
  --target wasm32-unknown-unknown \
  --release \
  -p bridge-canister \
  --features test-deployment

mkdir -p "$OUTPUT_DIR"
cp "$OUTPUT_DIR/build/wasm32-unknown-unknown/release/bridge_canister.wasm" "$OUTPUT_WASM"
printf '%s\n' "$V30_SOURCE_COMMIT" >"$OUTPUT_DIR/source-commit.txt"
printf '%s\n' "$OUTPUT_WASM"
