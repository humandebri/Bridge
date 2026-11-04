#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VALIDATION="$ROOT/scripts/production-validation.sh"
SCHEMA_CONSISTENCY_FIXTURE='{"schema_version":36,"expected_bridge_signer":"current-state-policy"}'
[[ "$SCHEMA_CONSISTENCY_FIXTURE" == *'"schema_version":36'* ]]

# Historical upgrade chains and handover recovery are no longer executable policy.
! grep -q 'production_validate_gate_b_source_chain' "$VALIDATION"
! grep -q 'production-canister-upgrade-receipt' "$VALIDATION"
! grep -q 'post-gate-a-policy-transition' "$VALIDATION"
! grep -q 'handover-recover\|pre_send_checkpoint' "$VALIDATION"

# The shared gate still enforces clean source, proofs, and reproducible artifacts.
grep -q 'production_require_clean_source' "$VALIDATION"
grep -q 'production_run_proof_gate' "$VALIDATION"
grep -q 'rebuild-release-artifacts.sh' "$VALIDATION"
grep -q -- '-u BRIDGE_CONFIRM_PRODUCTION_CANISTER_UPGRADE' "$VALIDATION"
grep -q -- '-u BRIDGE_CONFIRM_SNS_DAPP_REGISTRATION' "$VALIDATION"
grep -q -- '-u BRIDGE_CONFIRM_SNS_SAME_WASM_UPGRADE' "$VALIDATION"

# The one-time production Root addition is retired after handover.
[[ ! -e "$ROOT/scripts/production-handover-driver.sh" ]]
! grep -q 'execute-production-root-addition' "$ROOT/tools/bridge-profile/src/main.rs"

echo 'production driver policy contract: pass'
