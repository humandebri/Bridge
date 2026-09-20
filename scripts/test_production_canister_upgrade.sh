#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRIVER="$ROOT/scripts/production-canister-upgrade.sh"
PROFILE="$ROOT/tools/bridge-profile/src/main.rs"

bash -n "$DRIVER"
grep -q 'MODE" == check || "$MODE" == execute' "$DRIVER"
grep -q -- '--expected-current-wasm' "$DRIVER"
grep -q 'verify-production-current-state' "$DRIVER"
grep -q 'execute-production-canister-upgrade' "$DRIVER"
grep -q 'do not retry for 6 minutes' "$PROFILE"
grep -q 'send={send_error}; observation={observation_error}' "$PROFILE"
grep -q 'read_state_canister_module_hash' "$PROFILE"
grep -q 'read_state_canister_controllers' "$PROFILE"
grep -q 'call_with_verification' "$PROFILE"
grep -q 'ManagementInstallMode::Upgrade' "$PROFILE"
! grep -q -- '--checkpoint-evidence\|--preflight\|--receipt\| recover' "$DRIVER"
! grep -q 'checkpoint_evidence_sha256' "$PROFILE"
! grep -q 'mod production_checkpoint' "$PROFILE"
! grep -q 'ambiguous response.*exit 0\|verify-production-current-state.*CANDIDATE_WASM' "$DRIVER"

T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-upgrade-contract.XXXXXX")"
trap 'rm -rf "$T"' EXIT
printf wasm >"$T/candidate.wasm"
if BRIDGE_ICP_IDENTITY=production BRIDGE_RELEASE_BUNDLE="$T" \
  "$DRIVER" recover --wasm "$T/candidate.wasm" >/dev/null 2>&1; then
  echo "legacy recover mode was accepted" >&2; exit 1
fi
if BRIDGE_ICP_IDENTITY=production BRIDGE_RELEASE_BUNDLE="$T" \
  "$DRIVER" check --wasm "$T/candidate.wasm" --checkpoint-evidence "$T/old.json" >/dev/null 2>&1; then
  echo "legacy checkpoint argument was accepted" >&2; exit 1
fi
[[ -z "$(find "$T" -type f ! -name candidate.wasm -print -quit)" ]] || {
  echo "rejected upgrade invocation created an operation artifact" >&2; exit 1;
}
echo "production current-state upgrade contract: pass"
