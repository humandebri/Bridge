#!/usr/bin/env bash
# Fixed two-invocation Timelock activation. schedule never enables assets; execute does so after delay.
set -euo pipefail
SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"
: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing Gate B approval}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_ACTIVATION_PHASE:?set BRIDGE_ACTIVATION_PHASE=schedule or execute}"
: "${BRIDGE_BASE_ADMIN_ADDRESS:?missing hardware-wallet Base admin address}"
: "${BRIDGE_DFX_IDENTITY:?missing reviewed dfx identity}"
[[ "$BRIDGE_ACTIVATION_PHASE" == schedule || "$BRIDGE_ACTIVATION_PHASE" == execute ]] || { echo "invalid activation phase" >&2; exit 1; }
for tool in cast dfx python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
[[ -x "$SOURCE_ROOT/scripts/production-live-preflight.sh" ]] || { echo "reviewed live preflight is missing" >&2; exit 1; }
"$SOURCE_ROOT/scripts/production-live-preflight.sh" verify "$BRIDGE_RELEASE_BUNDLE"
production_validate_gate gate-b "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256"
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"
read -r RPC BRIDGE TIMELOCK DELAY CANISTER IC_HOST < <(python3 -c 'import json,sys;p=json.load(open(sys.argv[1]));print(p["base_rpc_url"],p["bridge_contract"],p["timelock"]["address"],p["timelock"]["minimum_delay_seconds"],p["bridge_canister_id"],p["ic_host"])' "$PROFILE")
D1="$(cast calldata 'unpauseDepositMints()')"; D2="$(cast calldata 'unpauseWithdrawals()')"
TARGETS="[$BRIDGE,$BRIDGE]"; VALUES='[0,0]'; PAYLOADS="[$D1,$D2]"; ZERO="0x$(printf '%064d' 0)"
GATE_A_HASH="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["gate_a_manifest_sha256"])' "$BRIDGE_RELEASE_BUNDLE/gate-a-receipt.json")"
[[ "$GATE_A_HASH" =~ ^[0-9a-fA-F]{64}$ ]] || { echo "invalid Gate A receipt hash" >&2; exit 1; }
SALT="0x$(printf '%s' "$GATE_A_HASH" | tr '[:upper:]' '[:lower:]')"
OP="$(cast call "$TIMELOCK" 'hashOperationBatch(address[],uint256[],bytes[],bytes32,bytes32)(bytes32)' "$TARGETS" "$VALUES" "$PAYLOADS" "$ZERO" "$SALT" --rpc-url "$RPC")"
if [[ "$BRIDGE_ACTIVATION_PHASE" == schedule ]]; then
  cast send "$TIMELOCK" 'scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)' "$TARGETS" "$VALUES" "$PAYLOADS" "$ZERO" "$SALT" "$DELAY" --rpc-url "$RPC" --ledger --from "$BRIDGE_BASE_ADMIN_ADDRESS"
  [[ "$(cast call "$TIMELOCK" 'isOperationPending(bytes32)(bool)' "$OP" --rpc-url "$RPC")" == true ]] || { echo "Timelock operation was not scheduled" >&2; exit 1; }
  echo "activation scheduled; rerun with a fresh Gate B bundle and BRIDGE_ACTIVATION_PHASE=execute after the Timelock delay" >&2
  exit 0
fi
[[ "$(cast call "$TIMELOCK" 'isOperationReady(bytes32)(bool)' "$OP" --rpc-url "$RPC")" == true ]] || { echo "Timelock operation is not ready" >&2; exit 1; }
: "${BRIDGE_RUNTIME_ADMIN_ADDRESS:?execute requires the hardware-wallet runtime administrator}"
EXPECTED_RUNTIME_ADMIN="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["runtime_administrator"])' "$PROFILE")"
[[ "$(printf '%s' "$BRIDGE_RUNTIME_ADMIN_ADDRESS" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$EXPECTED_RUNTIME_ADMIN" | tr '[:upper:]' '[:lower:]')" ]] || { echo "runtime administrator does not match the approved profile" >&2; exit 1; }
cast send "$TIMELOCK" 'executeBatch(address[],uint256[],bytes[],bytes32,bytes32)' "$TARGETS" "$VALUES" "$PAYLOADS" "$ZERO" "$SALT" --rpc-url "$RPC" --ledger --from "$BRIDGE_BASE_ADMIN_ADDRESS"
[[ "$(cast call "$BRIDGE" 'depositMintsPaused()(bool)' --rpc-url "$RPC")" == false && "$(cast call "$BRIDGE" 'withdrawalsPaused()(bool)' --rpc-url "$RPC")" == false ]] || { echo "Base activation did not complete" >&2; exit 1; }
compensate_base_pause() {
  local ic_pause_status deposit_pause_status withdrawal_pause_status base_confirm_status ic_confirm_status status_file
  status_file="$(mktemp "${TMPDIR:-/tmp}/bridge-compensation-status.XXXXXX")"
  set +e
  dfx canister call "$CANISTER" pause_new_deposits '()' --network "$IC_HOST" --identity "$BRIDGE_DFX_IDENTITY" --output json >/dev/null
  ic_pause_status=$?
  cast send "$BRIDGE" 'pauseDepositMints()' --rpc-url "$RPC" --ledger --from "$BRIDGE_RUNTIME_ADMIN_ADDRESS"
  deposit_pause_status=$?
  cast send "$BRIDGE" 'pauseWithdrawals()' --rpc-url "$RPC" --ledger --from "$BRIDGE_RUNTIME_ADMIN_ADDRESS"
  withdrawal_pause_status=$?
  python3 - "$PROFILE" <<'PY'
import json,subprocess,sys
p=json.load(open(sys.argv[1])); ok=[]
for item in p['rpc_providers']:
  rpc=item['url']
  try:
    b=json.loads(subprocess.check_output(['cast','block','safe','--rpc-url',rpc,'--json'],text=True)); h=str(int(str(b['number']),16) if str(b['number']).startswith('0x') else int(b['number']))
    d=subprocess.check_output(['cast','call',p['bridge_contract'],'depositMintsPaused()(bool)','--rpc-url',rpc,'--block',h],text=True).strip().lower()
    w=subprocess.check_output(['cast','call',p['bridge_contract'],'withdrawalsPaused()(bool)','--rpc-url',rpc,'--block',h],text=True).strip().lower()
    ok.append(d=='true' and w=='true')
  except Exception: ok.append(False)
raise SystemExit(0 if sum(ok)>=2 else 1)
PY
  base_confirm_status=$?
  dfx canister call "$CANISTER" get_bridge_status '()' --network "$IC_HOST" --output json >"$status_file"
  if [[ $? -eq 0 ]]; then
    python3 - "$status_file" <<'PY'
import json,sys
def values(v):
  if isinstance(v,dict): return ([v['deposits_paused']] if 'deposits_paused' in v else [])+sum((values(x) for x in v.values()),[])
  if isinstance(v,list): return sum((values(x) for x in v),[])
  return []
raise SystemExit(0 if values(json.load(open(sys.argv[1])))==[True] else 1)
PY
    ic_confirm_status=$?
  else ic_confirm_status=1
  fi
  set -e
  rm -f "$status_file"
  if [[ $ic_pause_status -eq 0 && $deposit_pause_status -eq 0 && $withdrawal_pause_status -eq 0 && $base_confirm_status -eq 0 && $ic_confirm_status -eq 0 ]]; then
    echo "INCIDENT: IC activation failed; Base asset flows were re-paused and require operator review" >&2
  else
    echo "INCIDENT: activation compensation incomplete (ic_pause=$ic_pause_status deposit_pause=$deposit_pause_status withdrawal_pause=$withdrawal_pause_status base_confirm=$base_confirm_status ic_confirm=$ic_confirm_status); invoke the emergency runbook immediately" >&2
  fi
  exit 1
}
RESUME_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/bridge-resume.XXXXXX")"
if ! dfx canister call "$CANISTER" resume_new_deposits '()' --network "$IC_HOST" --identity "$BRIDGE_DFX_IDENTITY" --output json >"$RESUME_OUTPUT" \
  || ! python3 -c 'import json,sys; v=json.load(open(sys.argv[1])); ok=[x for x in ([v] if not isinstance(v,dict) else [v]) if isinstance(x,dict) and "Ok" in x]; raise SystemExit(0 if len(ok)==1 else 1)' "$RESUME_OUTPUT"; then
  rm -f "$RESUME_OUTPUT"; compensate_base_pause
fi
rm -f "$RESUME_OUTPUT"
if ! dfx canister call "$CANISTER" get_bridge_status '()' --network "$IC_HOST" --output json | python3 -c '
import json,sys
def values(v):
    if isinstance(v,dict): return ([v["deposits_paused"]] if "deposits_paused" in v else []) + sum((values(x) for x in v.values()),[])
    if isinstance(v,list): return sum((values(x) for x in v),[])
    return []
raise SystemExit(0 if values(json.load(sys.stdin)) == [False] else 1)'; then
  compensate_base_pause
fi
