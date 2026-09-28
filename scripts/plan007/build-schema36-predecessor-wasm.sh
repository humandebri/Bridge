#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PREDECESSOR_REVISION="8a5e883ffb26ba8411a8199383d4f0e88f2ea532"
PREDECESSOR_ARCHIVE_SHA256="257448aa92529cab65ffdc371271ad4804e53fca8f8c9de5edd69bac578a41a6"
OUTPUT="$ROOT/target/test-deployment/predecessor-v36/bridge_canister.wasm"
SOURCE="$(mktemp -d "${TMPDIR:-/tmp}/bridge-schema36-predecessor.XXXXXX")"
ARCHIVE="$SOURCE/source.tar"
cleanup() {
  rm -rf "$SOURCE"
}
trap cleanup EXIT

git -C "$ROOT" cat-file -e "$PREDECESSOR_REVISION^{commit}"
git -C "$ROOT" archive --format=tar -o "$ARCHIVE" "$PREDECESSOR_REVISION"
ACTUAL_ARCHIVE_SHA256="$(shasum -a 256 "$ARCHIVE" | cut -d' ' -f1)"
test "$ACTUAL_ARCHIVE_SHA256" = "$PREDECESSOR_ARCHIVE_SHA256"
mkdir "$SOURCE/source"
tar -xf "$ARCHIVE" -C "$SOURCE/source"
mkdir -p "$ROOT/target/test-deployment/predecessor-v36"
mkdir -p "$ROOT/target/test-deployment/schema36-build"
ln -s "$ROOT/target/test-deployment/schema36-build" "$SOURCE/source/target"
CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 \
  "$SOURCE/source/scripts/plan007/build-staging-canister-wasm.sh" "$OUTPUT" >/dev/null
printf '%s\n' "$OUTPUT"
