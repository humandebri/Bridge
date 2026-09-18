#!/usr/bin/env bash
# Relay and verify an SNS-authorized DAO reactivation while joint control is retained.
set -Eeuo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing Gate B evidence hash}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_CURRENT_MODULE_SHA256:?missing current production module SHA-256}"
: "${BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:?missing operational config seal receipt}"
: "${BRIDGE_CONTROLLER_SCHEDULE_RECEIPT:?missing historical controller schedule receipt}"
: "${BRIDGE_CONTROLLER_ACTIVATION_RECEIPT:?missing historical controller execute receipt}"
: "${BRIDGE_DAO_ACTIVATION_PHASE:?set BRIDGE_DAO_ACTIVATION_PHASE=schedule or execute}"
: "${BRIDGE_DAO_ACTIVATION_STEP:?set BRIDGE_DAO_ACTIVATION_STEP=recover, relay, or confirm}"
: "${BRIDGE_DAO_ACTIVATION_SUBMISSION:?missing schema 4 SNS proposal submission receipt}"
: "${BRIDGE_DAO_ACTIVATION_ARTIFACT:?missing fixed activation artifact path}"

[[ "$BRIDGE_DAO_ACTIVATION_PHASE" == schedule || "$BRIDGE_DAO_ACTIVATION_PHASE" == execute ]] || {
  echo "invalid DAO activation phase" >&2; exit 1;
}
[[ "$BRIDGE_DAO_ACTIVATION_STEP" == recover || "$BRIDGE_DAO_ACTIVATION_STEP" == relay || "$BRIDGE_DAO_ACTIVATION_STEP" == confirm ]] || {
  echo "invalid DAO activation step" >&2; exit 1;
}
EXPECTED_CONFIRMATION=RELAY_SNS_SCHEDULE_ACTIVATION
[[ "$BRIDGE_DAO_ACTIVATION_STEP" == confirm ]] && EXPECTED_CONFIRMATION=CONFIRM_SNS_SCHEDULE_ACTIVATION
if [[ "$BRIDGE_DAO_ACTIVATION_PHASE" == execute ]]; then
  EXPECTED_CONFIRMATION=RELAY_SNS_EXECUTE_ACTIVATION
  [[ "$BRIDGE_DAO_ACTIVATION_STEP" == confirm ]] && EXPECTED_CONFIRMATION=CONFIRM_SNS_EXECUTE_ACTIVATION
fi
[[ "${BRIDGE_DAO_ACTIVATION_CONFIRMATION:-}" == "$EXPECTED_CONFIRMATION" ]] || {
  echo "DAO activation requires the phase- and step-specific confirmation token" >&2; exit 1;
}
for tool in node python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-dao-activation.XXXXXX")"
cleanup() {
  local status=$?
  trap - EXIT
  chmod -R u+w "$TMP" 2>/dev/null || true
  rm -rf "$TMP"
  exit "$status"
}
trap cleanup EXIT
mkdir -m 700 "$TMP/bundle"
production_freeze_bundle "$BRIDGE_RELEASE_BUNDLE" "$TMP/bundle"
BRIDGE_RELEASE_BUNDLE="$TMP/bundle"
for spec in \
  "BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:seal.json:operational config seal receipt" \
  "BRIDGE_CONTROLLER_SCHEDULE_RECEIPT:controller-schedule.json:historical controller schedule receipt" \
  "BRIDGE_CONTROLLER_ACTIVATION_RECEIPT:controller-execute.json:historical controller execute receipt" \
  "BRIDGE_DAO_ACTIVATION_SUBMISSION:submission.json:SNS proposal submission receipt"; do
  IFS=: read -r variable target label <<<"$spec"
  production_freeze_receipt "${!variable}" "$TMP/$target" "$label"
  printf -v "$variable" '%s' "$TMP/$target"
  export "${variable?}"
done
if [[ "$BRIDGE_DAO_ACTIVATION_PHASE" == execute ]]; then
  : "${BRIDGE_DAO_PRIOR_SCHEDULE_RECEIPT:?execute requires the schema 5 schedule receipt}"
  production_freeze_receipt "$BRIDGE_DAO_PRIOR_SCHEDULE_RECEIPT" "$TMP/prior-schedule.json" "DAO schedule receipt"
  BRIDGE_DAO_PRIOR_SCHEDULE_RECEIPT="$TMP/prior-schedule.json"
fi

read -r BRIDGE_CANISTER_ID BRIDGE_SNS_ROOT_CANISTER_ID IC_HOST BRIDGE_PRODUCTION_CONTROLLER_PRINCIPAL < <(python3 -c '
import json,sys
p=json.load(open(sys.argv[1],encoding="utf-8"))
print(p["bridge_canister_id"],p["root_canister_id"],p["ic_host"],p["pause_principal"])
' "$BRIDGE_RELEASE_BUNDLE/profile.json")
export BRIDGE_CANISTER_ID BRIDGE_SNS_ROOT_CANISTER_ID IC_HOST BRIDGE_PRODUCTION_CONTROLLER_PRINCIPAL
export BRIDGE_PRODUCTION_INSTALLER_IDENTITY=production BRIDGE_DAO_JOINT_CONTROL=1
AUTHORIZATION="${BRIDGE_DAO_ACTIVATION_ARTIFACT}.sns-authorization.json"
BINDING="${BRIDGE_DAO_ACTIVATION_ARTIFACT}.sns-binding.json"
CLI=(node --no-warnings --experimental-strip-types "$SOURCE_ROOT/tools/governance-relayer/cli.ts")
PROFILE=(cargo run --locked --quiet --release --manifest-path "$SOURCE_ROOT/Cargo.toml" -p bridge-profile --)
"${PROFILE[@]}" verify-production-current-state "$BRIDGE_RELEASE_BUNDLE/profile.json" \
  "$BRIDGE_PRODUCTION_CONTROLLER_PRINCIPAL" "$BRIDGE_CURRENT_MODULE_SHA256" joint

freeze_relay_inputs() {
  production_freeze_receipt "$BRIDGE_DAO_ACTIVATION_ARTIFACT" "$TMP/artifact.json" "SNS activation artifact"
  production_freeze_receipt "$AUTHORIZATION" "$TMP/authorization.json" "SNS activation authorization"
  production_freeze_receipt "$BINDING" "$TMP/binding.json" "SNS activation binding"
}

case "$BRIDGE_DAO_ACTIVATION_STEP" in
  recover)
    [[ ! -e "$BRIDGE_DAO_ACTIVATION_ARTIFACT" && ! -e "$AUTHORIZATION" && ! -e "$BINDING" ]] || {
      echo "SNS activation recovery outputs already exist; inspect them instead of resending" >&2; exit 1;
    }
    unset IC_IDENTITY_PEM
    "${CLI[@]}" recover-sns-activation \
      --phase "$BRIDGE_DAO_ACTIVATION_PHASE" \
      --submission-file "$BRIDGE_DAO_ACTIVATION_SUBMISSION" \
      --artifact-file "$BRIDGE_DAO_ACTIVATION_ARTIFACT" \
      --authorization-file "$AUTHORIZATION" --binding-file "$BINDING"
    ;;
  relay)
    : "${BASE_RPC_URL:?missing Base RPC URL for raw relay}"
    freeze_relay_inputs
    unset IC_IDENTITY_PEM
    "${CLI[@]}" relay --artifact-file "$TMP/artifact.json" \
      --authorization-file "$TMP/authorization.json" --binding-file "$TMP/binding.json"
    ;;
  confirm)
    : "${BRIDGE_CONFIRMATION_RELAYER_PEM:?missing confirmation relayer identity PEM}"
    : "${BRIDGE_DAO_CONFIRMATION_RECEIPT:?missing Base confirmation receipt output}"
    : "${BRIDGE_DAO_ACTIVATION_RECEIPT:?missing schema 5 DAO activation receipt output}"
    [[ -f "$BRIDGE_CONFIRMATION_RELAYER_PEM" && ! -L "$BRIDGE_CONFIRMATION_RELAYER_PEM" ]] || {
      echo "confirmation relayer PEM must be an ordinary file" >&2; exit 1;
    }
    freeze_relay_inputs
    export IC_IDENTITY_PEM="$BRIDGE_CONFIRMATION_RELAYER_PEM"
    "${CLI[@]}" confirm --artifact-file "$TMP/artifact.json" \
      --authorization-file "$TMP/authorization.json" --binding-file "$TMP/binding.json" \
      --receipt-file "$BRIDGE_DAO_CONFIRMATION_RECEIPT"
    "${CLI[@]}" refresh-attestation
    PRIOR="${BRIDGE_DAO_PRIOR_SCHEDULE_RECEIPT:--}"
    "${PROFILE[@]}" verify-activation "$BRIDGE_DAO_ACTIVATION_PHASE" \
      "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_DAO_ACTIVATION_SUBMISSION" "$PRIOR" \
      "$BRIDGE_DAO_ACTIVATION_RECEIPT"
    ;;
esac
