#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRIVER="$ROOT/scripts/production-handover-driver.sh"
PROFILE="$ROOT/tools/bridge-profile/src/main.rs"
CANDIDATE_DRIVER="${BRIDGE_CANDIDATE_SCRIPTS:-$ROOT/scripts}/production-handover-driver.sh"

# Trusted CI overlays these policy scripts, so inspect the candidate driver
# separately before accepting retirement of the one-time Root addition.
if [[ ! -e "$CANDIDATE_DRIVER" && ! -L "$CANDIDATE_DRIVER" ]]; then
  if grep -q 'execute-production-root-addition\|execute_production_root_addition\|preserved_across_root_addition' "$PROFILE" \
    || grep -q -- '"--add-controller"' "$PROFILE"; then
    echo 'Root addition is only partially retired' >&2; exit 1
  fi
  grep -q 'root_state.dapps.iter().filter(|id| \*\*id == bridge).count()' "$PROFILE"
  grep -q 'sole-unregistered, joint-unregistered, or root-registered' "$PROFILE"
  echo 'retired production Root addition contract: pass'
  exit 0
fi
[[ -f "$CANDIDATE_DRIVER" && ! -L "$CANDIDATE_DRIVER" ]] || {
  echo 'candidate Root addition driver is not a regular file' >&2; exit 1;
}

bash -n "$DRIVER"
grep -q 'STAGE_KINIC_SNS_ROOT_CO_CONTROLLER' "$DRIVER"
grep -q 'execute-production-root-addition' "$DRIVER"
grep -q -- '"--add-controller"' "$PROFILE"
grep -q 'preserved_across_root_addition' "$PROFILE"
grep -q 'do not retry for 6 minutes' "$PROFILE"
grep -q 'root_state.dapps.iter().filter(|id| \*\*id == bridge).count()' "$PROFILE"
grep -q 'sole-unregistered, joint-unregistered, or root-registered' "$PROFILE"
! grep -q 'BRIDGE_HANDOVER_EVIDENCE_FILE\|BRIDGE_HANDOVER_MODE\|pre_send_checkpoint\|recover' "$DRIVER"
! grep -q 'BRIDGE_CHECKPOINT_EVIDENCE\|production-checkpoint' "$DRIVER"
! grep -q 'icp canister settings update' "$DRIVER"

if BRIDGE_RELEASE_BUNDLE=/nonexistent BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_CONFIRMATION=WRONG "$DRIVER" >/dev/null 2>&1; then
  echo "handover accepted the wrong confirmation" >&2; exit 1
fi
echo "production current-state handover contract: pass"
