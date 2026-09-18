#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DRIVER="$ROOT/scripts/production-handover-driver.sh"
PROFILE="$ROOT/tools/bridge-profile/src/main.rs"

bash -n "$DRIVER"
grep -q 'STAGE_KINIC_SNS_ROOT_CO_CONTROLLER' "$DRIVER"
grep -q -- '--add-controller "$SNS_ROOT"' "$DRIVER"
grep -q 'verify-production-current-state.*sole' "$DRIVER"
grep -q 'verify-production-current-state.*joint' "$DRIVER"
grep -q 'do not retry for 6 minutes' "$DRIVER"
grep -q 'root_state.dapps.contains(&bridge)' "$PROFILE"
grep -q 'controller_mode.*sole.*joint\|controller mode must be sole or joint' "$PROFILE"
! grep -q 'BRIDGE_HANDOVER_EVIDENCE_FILE\|BRIDGE_HANDOVER_MODE\|pre_send_checkpoint\|recover' "$DRIVER"
! grep -q 'BRIDGE_CHECKPOINT_EVIDENCE\|production-checkpoint' "$DRIVER"

if BRIDGE_RELEASE_BUNDLE=/nonexistent BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_CONFIRMATION=WRONG "$DRIVER" >/dev/null 2>&1; then
  echo "handover accepted the wrong confirmation" >&2; exit 1
fi
echo "production current-state handover contract: pass"
