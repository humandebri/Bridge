#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-dao-reactivation-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/source/scripts" "$T/source/tools/governance-relayer" "$T/bundle" "$T/bin"
cp "$ROOT/scripts/production-dao-reactivation-driver.sh" "$T/source/scripts/"
cat >"$T/source/scripts/production-validation.sh" <<'SH'
export TRACE
production_freeze_bundle() { cp -R "$1"/. "$2"/; }
production_freeze_receipt() { cp "$1" "$2"; }
production_validate_gate() { printf 'gate %s\n' "$*" >>"$TRACE"; }
SH
printf '{}\n' >"$T/source/tools/governance-relayer/cli.ts"
cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"lb5i5-ziaaa-aaaar-qcgwq-cai","root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","ic_host":"https://icp-api.io","pause_principal":"lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe"}
JSON
printf '{}\n' >"$T/bundle/release-manifest.json"
for name in seal controller-schedule controller-execute submission; do
  printf '{"name":"%s"}\n' "$name" >"$T/$name.json"
done
printf 'private\n' >"$T/relayer.pem"
cat >"$T/bin/node" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'node %s\n' "$*" >>"$TRACE"
case " $* " in
  *' recover-sns-activation '*)
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --artifact-file) artifact="$2"; shift 2 ;;
        --authorization-file) authorization="$2"; shift 2 ;;
        --binding-file) binding="$2"; shift 2 ;;
        *) shift ;;
      esac
    done
    printf '{"operation_id":"2"}\n' >"$artifact"
    printf '{"kind":"sns-activation-proposal-authorization"}\n' >"$authorization"
    printf '{"schema_version":1}\n' >"$binding"
    ;;
  *' confirm '*)
    while [[ $# -gt 0 ]]; do
      if [[ "$1" == --receipt-file ]]; then printf '{}\n' >"$2"; break; fi
      shift
    done
    ;;
esac
SH
cat >"$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'cargo %s\n' "$*" >>"$TRACE"
if [[ " $* " == *' verify-activation '* ]]; then printf '{"schema_version":5}\n' >"${@: -1}"; fi
SH
chmod +x "$T/bin/node" "$T/bin/cargo" "$T/source/scripts/production-dao-reactivation-driver.sh"
export PATH="$T/bin:$PATH" TRACE="$T/trace"

common=(
  BRIDGE_GATE_B_MANIFEST_SHA256="$(printf 'a%.0s' {1..64})"
  BRIDGE_RELEASE_BUNDLE="$T/bundle"
  BRIDGE_CURRENT_MODULE_SHA256="$(printf 'b%.0s' {1..64})"
  BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT="$T/seal.json"
  BRIDGE_CONTROLLER_SCHEDULE_RECEIPT="$T/controller-schedule.json"
  BRIDGE_CONTROLLER_ACTIVATION_RECEIPT="$T/controller-execute.json"
  BRIDGE_DAO_ACTIVATION_PHASE=schedule
  BRIDGE_DAO_ACTIVATION_SUBMISSION="$T/submission.json"
  BRIDGE_DAO_ACTIVATION_ARTIFACT="$T/artifact.json"
)

env "${common[@]}" BRIDGE_DAO_ACTIVATION_STEP=recover \
  BRIDGE_DAO_ACTIVATION_CONFIRMATION=RELAY_SNS_SCHEDULE_ACTIVATION \
  "$T/source/scripts/production-dao-reactivation-driver.sh" >/dev/null
rg -q 'recover-sns-activation.*--submission-file' "$TRACE"
! rg -q -- '--preparation-file' "$TRACE"
[[ -f "$T/artifact.json.sns-authorization.json" && -f "$T/artifact.json.sns-binding.json" ]]

env "${common[@]}" BRIDGE_DAO_ACTIVATION_STEP=relay \
  BRIDGE_DAO_ACTIVATION_CONFIRMATION=RELAY_SNS_SCHEDULE_ACTIVATION BASE_RPC_URL=https://example.invalid \
  "$T/source/scripts/production-dao-reactivation-driver.sh" >/dev/null
rg -q ' relay .*--authorization-file.*--binding-file' "$TRACE"

env "${common[@]}" BRIDGE_DAO_ACTIVATION_STEP=confirm \
  BRIDGE_DAO_ACTIVATION_CONFIRMATION=CONFIRM_SNS_SCHEDULE_ACTIVATION \
  BRIDGE_CONFIRMATION_RELAYER_PEM="$T/relayer.pem" \
  BRIDGE_DAO_CONFIRMATION_RECEIPT="$T/confirmation.json" \
  BRIDGE_DAO_ACTIVATION_RECEIPT="$T/activation-receipt.json" \
  "$T/source/scripts/production-dao-reactivation-driver.sh" >/dev/null
rg -q ' refresh-attestation' "$TRACE"
rg -q 'verify-activation schedule' "$TRACE"
[[ -f "$T/confirmation.json" && -f "$T/activation-receipt.json" ]]

if env "${common[@]}" BRIDGE_DAO_ACTIVATION_STEP=relay \
  BRIDGE_DAO_ACTIVATION_CONFIRMATION=CONFIRM_SNS_SCHEDULE_ACTIVATION BASE_RPC_URL=https://example.invalid \
  "$T/source/scripts/production-dao-reactivation-driver.sh" >/dev/null 2>&1; then
  echo "DAO driver accepted the wrong phase/step confirmation" >&2; exit 1
fi
