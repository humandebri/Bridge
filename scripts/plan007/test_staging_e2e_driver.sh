#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TASK_TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TASK_TMP_DIR"' EXIT

TRACE="$TASK_TMP_DIR/python.trace"
FAKE_BIN="$TASK_TMP_DIR/bin"
mkdir -p "$FAKE_BIN"
printf '%s\n' '#!/usr/bin/env bash' 'printf "%s\\n" "$*" >> "$STAGING_DRIVER_TRACE"' > "$FAKE_BIN/python3"
chmod +x "$FAKE_BIN/python3"
printf '{}\n' > "$TASK_TMP_DIR/local-e2e-v8.json"

if env PATH="$FAKE_BIN:$PATH" STAGING_DRIVER_TRACE="$TRACE" BRIDGE_STAGING_E2E_MANIFEST="$TASK_TMP_DIR/manifest.json" bash "$ROOT/scripts/plan007/staging-e2e-driver.sh" init 2>"$TASK_TMP_DIR/missing.err"; then
  echo "init unexpectedly accepted a missing BRIDGE_STAGING_LOCAL_EVIDENCE" >&2
  exit 1
fi
grep -F "BRIDGE_STAGING_LOCAL_EVIDENCE must name" "$TASK_TMP_DIR/missing.err" >/dev/null
[[ ! -e "$TRACE" ]]

env PATH="$FAKE_BIN:$PATH" STAGING_DRIVER_TRACE="$TRACE" BRIDGE_STAGING_LOCAL_EVIDENCE="$TASK_TMP_DIR/local-e2e-v8.json" BRIDGE_STAGING_E2E_MANIFEST="$TASK_TMP_DIR/manifest.json" bash "$ROOT/scripts/plan007/staging-e2e-driver.sh" init

CANONICAL_LOCAL_EVIDENCE="$(realpath "$TASK_TMP_DIR/local-e2e-v8.json")"
grep -F "sepolia_e2e.py init $TASK_TMP_DIR/manifest.json $CANONICAL_LOCAL_EVIDENCE" "$TRACE" >/dev/null

if env PATH="$FAKE_BIN:$PATH" STAGING_DRIVER_TRACE="$TRACE" BRIDGE_STAGING_LOCAL_EVIDENCE="$ROOT/deployments/sepolia-staging/evidence/local-e2e.json" BRIDGE_STAGING_E2E_MANIFEST="$TASK_TMP_DIR/manifest.json" bash "$ROOT/scripts/plan007/staging-e2e-driver.sh" init 2>"$TASK_TMP_DIR/history.err"; then
  echo "init unexpectedly accepted checked-in v7 history" >&2
  exit 1
fi
grep -F "checked-in v7 evidence is history-only" "$TASK_TMP_DIR/history.err" >/dev/null

ln -s "$ROOT/deployments/sepolia-staging/evidence/local-e2e.json" "$TASK_TMP_DIR/repo-history-link.json"
if env PATH="$FAKE_BIN:$PATH" STAGING_DRIVER_TRACE="$TRACE" BRIDGE_STAGING_LOCAL_EVIDENCE="$TASK_TMP_DIR/repo-history-link.json" BRIDGE_STAGING_E2E_MANIFEST="$TASK_TMP_DIR/manifest.json" bash "$ROOT/scripts/plan007/staging-e2e-driver.sh" init 2>"$TASK_TMP_DIR/symlink.err"; then
  echo "init unexpectedly accepted an external symlink to checked-in v7 history" >&2
  exit 1
fi
grep -F "checked-in v7 evidence is history-only" "$TASK_TMP_DIR/symlink.err" >/dev/null

echo "staging E2E driver tests passed"
