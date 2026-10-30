#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-root-reactivation-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/repo/scripts" "$T/repo/tools/governance-relayer" "$T/bin" "$T/bundle"
cp "$ROOT/scripts/production-root-reactivation-driver.sh" "$T/repo/scripts/"
cat >"$T/repo/scripts/production-validation.sh" <<'SH'
production_require_clean_source() { printf '%s\n' clean >>"$TEST_LOG"; }
production_run_proof_gate() { [[ -z "${BRIDGE_CONFIRM_ASSET_ACCEPTANCE:-}" ]]; printf '%s\n' proofs >>"$TEST_LOG"; }
production_freeze_bundle() { cp -R "$1/." "$2/"; }
production_freeze_receipt() { cp "$1" "$2"; }
SH
cat >"$T/bin/git" <<'SH'
#!/usr/bin/env bash
if [[ "$3" == rev-parse ]]; then printf '%s\n' 1111111111111111111111111111111111111111; else printf '%s\n' fake-tree; fi
SH
cat >"$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
printf 'cargo:%s\n' "$*" >>"$TEST_LOG"
if [[ "$*" == *verify-root-activation-pending* && "${TEST_REJECT_PENDING:-0}" == 1 ]]; then exit 1; fi
SH
cat >"$T/bin/node" <<'SH'
#!/usr/bin/env bash
printf 'node:%s\n' "$*" >>"$TEST_LOG"
if [[ "$*" == *' relay '* && -n "${IC_IDENTITY_PEM:-}" ]]; then exit 1; fi
if [[ "$*" == *' confirm '* ]]; then
  [[ -n "${IC_IDENTITY_PEM:-}" && -f "$IC_IDENTITY_PEM" ]] || exit 1
  while [[ $# -gt 0 ]]; do
    if [[ "$1" == --receipt-file ]]; then shift; printf '%s\n' '{}' >"$1"; break; fi
    shift
  done
fi
SH
chmod +x "$T/bin/git" "$T/bin/cargo" "$T/bin/node"
printf '%s\n' '{"bridge_canister_id":"lb5i5-ziaaa-aaaar-qcgwq-cai","ic_host":"https://icp-api.io"}' >"$T/bundle/profile.json"
printf '%s\n' '{}' >"$T/submission.json"
printf '%s\n' '{}' >"$T/artifact.json"
GATE="$(printf '11%.0s' {1..32})"
printf '{"phase":"schedule","gate_b_manifest_sha256":"%s"}\n' "$GATE" >"$T/evidence.json"
export PATH="$T/bin:$PATH" TEST_LOG="$T/log" TMPDIR="$T"
export BRIDGE_RELEASE_BUNDLE="$T/bundle" BRIDGE_ACTIVATION_PHASE=schedule BRIDGE_ACTIVATION_STEP=relay
export BRIDGE_ACTIVATION_SUBMISSION="$T/submission.json" BRIDGE_ACTIVATION_ARTIFACT="$T/artifact.json"
export BRIDGE_ACTIVATION_EVIDENCE="$T/evidence.json" BRIDGE_CURRENT_MODULE_SHA256="$GATE" BRIDGE_GATE_B_MANIFEST_SHA256="$GATE"
export BRIDGE_CONFIRM_ASSET_ACCEPTANCE=SCHEDULE_PRODUCTION_ASSET_ACTIVATION IC_IDENTITY_PEM=must-be-unset
bash "$T/repo/scripts/production-root-reactivation-driver.sh"
python3 - "$TEST_LOG" <<'PY'
import sys
lines=open(sys.argv[1]).read().splitlines()
proof=lines.index('proofs')
compare=next(i for i,line in enumerate(lines) if 'verify-root-activation-pending' in line and ' compare ' in line)
relay=next(i for i,line in enumerate(lines) if line.startswith('node:') and ' relay ' in line)
assert proof < compare < relay
PY
: >"$TEST_LOG"
if TEST_REJECT_PENDING=1 bash "$T/repo/scripts/production-root-reactivation-driver.sh"; then
  echo 'rejected live state unexpectedly proceeded' >&2; exit 1
fi
if rg -q '^node:' "$TEST_LOG"; then echo 'relay ran after failed verification' >&2; exit 1; fi
: >"$TEST_LOG"
printf '%s\n' '{"phase":"execute","gate_b_manifest_sha256":"bad"}' >"$T/evidence.json"
if bash "$T/repo/scripts/production-root-reactivation-driver.sh"; then
  echo 'mismatched approved evidence unexpectedly proceeded' >&2; exit 1
fi
if rg -q '^node:' "$TEST_LOG"; then echo 'relay ran after evidence mismatch' >&2; exit 1; fi
: >"$TEST_LOG"
printf '{"phase":"schedule","gate_b_manifest_sha256":"%s"}\n' "$GATE" >"$T/evidence.json"
printf '%s\n' 'test-only identity' >"$T/relayer.pem"
export BRIDGE_ACTIVATION_STEP=confirm BRIDGE_CONFIRMATION_RELAYER_PEM="$T/relayer.pem"
export BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT="$T/confirmation.json" BRIDGE_ACTIVATION_VERIFIED_RECEIPT="$T/verified.json"
bash "$T/repo/scripts/production-root-reactivation-driver.sh"
python3 - "$TEST_LOG" <<'PY'
import sys
lines=open(sys.argv[1]).read().splitlines()
compare=next(i for i,line in enumerate(lines) if 'verify-root-activation-pending' in line and ' compare ' in line)
confirm=next(i for i,line in enumerate(lines) if line.startswith('node:') and ' confirm ' in line)
verify=next(i for i,line in enumerate(lines) if 'verify-root-activation schedule' in line)
assert lines.index('proofs') < compare < confirm < verify
PY
: >"$TEST_LOG"
export BRIDGE_ACTIVATION_PHASE=execute BRIDGE_CONFIRM_ASSET_ACCEPTANCE=UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE
export BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT="$T/execute-confirmation.json" BRIDGE_ACTIVATION_VERIFIED_RECEIPT="$T/execute-verified.json"
if bash "$T/repo/scripts/production-root-reactivation-driver.sh"; then
  echo 'execute confirmation unexpectedly accepted a missing schedule receipt' >&2; exit 1
fi
if rg -q '^node:' "$TEST_LOG"; then echo 'confirmation ran without a schedule receipt' >&2; exit 1; fi
printf '%s\n' 'Root-only reactivation driver checks passed'
