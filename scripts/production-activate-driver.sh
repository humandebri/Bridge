#!/usr/bin/env bash
# Fixed SNS/Canister activation entrypoint. This driver never signs an EVM transaction.
set -Eeuo pipefail

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing Gate B evidence hash}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_ACTIVATION_PHASE:?set BRIDGE_ACTIVATION_PHASE=schedule or execute}"
: "${BRIDGE_ACTIVATION_SUBMISSION_OUT:?missing activation submission output}"
: "${BRIDGE_SNS_IDENTITY:?missing SNS proposer identity name}"
: "${BRIDGE_CONFIRMATION_RELAYER_IDENTITY:?missing confirmation relayer ICP identity name}"
: "${BRIDGE_SNS_NEURON_SUBACCOUNT:?missing SNS proposer neuron subaccount}"
: "${BRIDGE_SNS_PROPOSER_PRINCIPAL:?missing SNS proposer principal}"

[[ "$BRIDGE_ACTIVATION_PHASE" == schedule || "$BRIDGE_ACTIVATION_PHASE" == execute ]] || {
  echo "invalid activation phase" >&2
  exit 1
}
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

production_validate_gate gate-b "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256"

exec "$SOURCE_ROOT/scripts/production-activation-proposal.sh" \
  "$BRIDGE_ACTIVATION_PHASE" "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" \
  "$BRIDGE_ACTIVATION_SUBMISSION_OUT" "$BRIDGE_SNS_IDENTITY" \
  "$BRIDGE_SNS_NEURON_SUBACCOUNT" "$BRIDGE_SNS_PROPOSER_PRINCIPAL"
