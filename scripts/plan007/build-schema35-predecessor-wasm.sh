#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PREDECESSOR_REVISION="e0b426e7465531d2e572b5b741509f1889e6def8"
OUTPUT="$ROOT/target/test-deployment/predecessor-v35/bridge_canister.wasm"
SOURCE="$(mktemp -d "${TMPDIR:-/tmp}/bridge-schema35-predecessor.XXXXXX")"
cleanup() {
  rm -rf "$SOURCE"
}
trap cleanup EXIT

git -C "$ROOT" cat-file -e "$PREDECESSOR_REVISION^{commit}"
git -C "$ROOT" archive "$PREDECESSOR_REVISION" | tar -x -C "$SOURCE"
mkdir -p "$ROOT/target/test-deployment/predecessor-v35"
mkdir -p "$ROOT/target/test-deployment/schema35-build"
ln -s "$ROOT/target/test-deployment/schema35-build" "$SOURCE/target"
"$SOURCE/scripts/plan007/build-staging-canister-wasm.sh" "$OUTPUT" >/dev/null
printf '%s\n' "$OUTPUT"
