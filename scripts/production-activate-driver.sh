#!/usr/bin/env bash
# Fixed two-invocation Timelock activation. schedule never enables assets; execute does so after delay.
set -Eeuo pipefail
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
: "${BRIDGE_TIMELOCK_CANCELLER_ADDRESS:?execute requires the independent hardware-wallet Timelock canceller}"
EXPECTED_RUNTIME_ADMIN="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["runtime_administrator"])' "$PROFILE")"
EXPECTED_CANCELLER="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["timelock"]["canceller"])' "$PROFILE")"
[[ "$(printf '%s' "$BRIDGE_RUNTIME_ADMIN_ADDRESS" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$EXPECTED_RUNTIME_ADMIN" | tr '[:upper:]' '[:lower:]')" ]] || { echo "runtime administrator does not match the approved profile" >&2; exit 1; }
[[ "$(printf '%s' "$BRIDGE_TIMELOCK_CANCELLER_ADDRESS" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$EXPECTED_CANCELLER" | tr '[:upper:]' '[:lower:]')" ]] || { echo "Timelock canceller does not match the approved profile" >&2; exit 1; }
confirm_canonical_state() {
  python3 - "$PROFILE" "$1" "$2" "$3" "$OP" <<'PY'
import collections,json,subprocess,sys
p=json.load(open(sys.argv[1])); expected=sys.argv[2].lower(); require_terminal=sys.argv[3].lower()=='true'
transactions=[x for x in sys.argv[4].split(',') if x]; op=sys.argv[5]; observations=[]
def parse_number(value):
 return int(str(value),16) if str(value).startswith('0x') else int(value)
for item in p['rpc_providers']:
  rpc=item['url']
  try:
    block=json.loads(subprocess.check_output(['cast','block','safe','--rpc-url',rpc,'--json'],text=True))
    number=int(str(block['number']),16) if str(block['number']).startswith('0x') else int(block['number'])
    observations.append((number,str(block['hash']).lower(),rpc))
  except Exception: pass
pairs=collections.Counter((number,block_hash) for number,block_hash,_ in observations)
if not pairs: raise SystemExit('no Safe block observations')
(number,block_hash),count=pairs.most_common(1)[0]
if count<2: raise SystemExit('no 2-of-3 canonical Safe block agreement')
agreeing=[rpc for n,h,rpc in observations if (n,h)==(number,block_hash)]
matched=0
for rpc in agreeing:
  try:
    canonical=json.loads(subprocess.check_output(['cast','block',str(number),'--rpc-url',rpc,'--json'],text=True))
    if str(canonical.get('hash','')).lower()!=block_hash: continue
    selector=json.dumps({'blockHash':block_hash,'requireCanonical':True},separators=(',',':'))
    def call(address,sig,*call_args):
      data=subprocess.check_output(['cast','calldata',sig,*call_args],text=True).strip()
      request=json.dumps({'to':address,'data':data},separators=(',',':'))
      raw=subprocess.check_output(['cast','rpc','--rpc-url',rpc,'eth_call',request,selector],text=True).strip().strip('"')
      return subprocess.check_output(['cast','decode-abi',sig,raw],text=True).strip().lower()
    deposit=call(p['bridge_contract'],'depositMintsPaused()(bool)')
    withdrawal=call(p['bridge_contract'],'withdrawalsPaused()(bool)')
    done=call(p['timelock']['address'],'isOperationDone(bytes32)(bool)',op)=='true'
    pending=call(p['timelock']['address'],'isOperationPending(bytes32)(bool)',op)=='true'
    ready=call(p['timelock']['address'],'isOperationReady(bytes32)(bool)',op)=='true'
    terminal=done or (not pending and not ready)
    receipts_ok=True
    for tx in transactions:
      receipt=json.loads(subprocess.check_output(['cast','receipt',tx,'--rpc-url',rpc,'--json'],text=True))
      receipt_number=parse_number(receipt.get('blockNumber',-1)); receipt_hash=str(receipt.get('blockHash','')).lower()
      if parse_number(receipt.get('status',0))!=1 or receipt_number>number:
        receipts_ok=False; break
      receipt_block=json.loads(subprocess.check_output(['cast','block',str(receipt_number),'--rpc-url',rpc,'--json'],text=True))
      if str(receipt_block.get('hash','')).lower()!=receipt_hash:
        receipts_ok=False; break
    final=json.loads(subprocess.check_output(['cast','block',str(number),'--rpc-url',rpc,'--json'],text=True))
    matched += deposit==expected and withdrawal==expected and (terminal or not require_terminal) and receipts_ok and str(final.get('hash','')).lower()==block_hash
  except Exception: pass
raise SystemExit(0 if matched>=2 else 'activation state, receipts, and operation lack 2-of-3 agreement at one canonical Safe block')
PY
}
poll_canonical_activation() {
  local expected="$1" require_terminal="$2" transactions="${3:-}" deadline now
  local timeout="${BRIDGE_CANONICAL_CONFIRM_TIMEOUT_SECONDS:-900}"
  local interval="${BRIDGE_CANONICAL_CONFIRM_POLL_SECONDS:-5}"
  [[ "$timeout" =~ ^[1-9][0-9]*$ && "$interval" =~ ^[1-9][0-9]*$ ]] || {
    echo "canonical confirmation timeout and poll interval must be positive integers" >&2; return 1;
  }
  deadline=$(( $(date +%s) + timeout ))
  while :; do
    confirm_canonical_state "$expected" "$require_terminal" "$transactions" && return 0
    now=$(date +%s)
    (( now < deadline )) || return 1
    sleep "$interval"
  done
}
compensate_base_pause() {
  local ic_pause_status cancel_status operation_status deposit_pause_status withdrawal_pause_status base_confirm_status ic_confirm_status status_file
  local cancel_output deposit_output withdrawal_output deposit_tx="" withdrawal_tx=""
  status_file="$(mktemp "${TMPDIR:-/tmp}/bridge-compensation-status.XXXXXX")"
  cancel_output="$(mktemp "${TMPDIR:-/tmp}/bridge-cancel.XXXXXX")"
  deposit_output="$(mktemp "${TMPDIR:-/tmp}/bridge-deposit-pause.XXXXXX")"
  withdrawal_output="$(mktemp "${TMPDIR:-/tmp}/bridge-withdrawal-pause.XXXXXX")"
  set +e
  dfx canister call "$CANISTER" pause_new_deposits '()' --network "$IC_HOST" --identity "$BRIDGE_DFX_IDENTITY" --output json >/dev/null
  ic_pause_status=$?
  cast send "$TIMELOCK" 'cancel(bytes32)' "$OP" --rpc-url "$RPC" --ledger --from "$BRIDGE_TIMELOCK_CANCELLER_ADDRESS" --json >"$cancel_output"
  cancel_status=$?
  cast send "$BRIDGE" 'pauseDepositMints()' --rpc-url "$RPC" --ledger --from "$BRIDGE_RUNTIME_ADMIN_ADDRESS" --json >"$deposit_output"
  deposit_pause_status=$?
  cast send "$BRIDGE" 'pauseWithdrawals()' --rpc-url "$RPC" --ledger --from "$BRIDGE_RUNTIME_ADMIN_ADDRESS" --json >"$withdrawal_output"
  withdrawal_pause_status=$?
  if [[ $deposit_pause_status -eq 0 ]]; then deposit_tx="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("transactionHash",""))' "$deposit_output")"; fi
  if [[ $withdrawal_pause_status -eq 0 ]]; then withdrawal_tx="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("transactionHash",""))' "$withdrawal_output")"; fi
  if [[ ! "$deposit_tx" =~ ^0x[0-9a-fA-F]{64}$ || ! "$withdrawal_tx" =~ ^0x[0-9a-fA-F]{64}$ ]]; then
    base_confirm_status=1
  else
    poll_canonical_activation true true "$deposit_tx,$withdrawal_tx"
    base_confirm_status=$?
  fi
  operation_status=$base_confirm_status
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
  rm -f "$status_file" "$cancel_output" "$deposit_output" "$withdrawal_output"
  if [[ $ic_pause_status -eq 0 && $deposit_pause_status -eq 0 && $withdrawal_pause_status -eq 0 && $base_confirm_status -eq 0 && $operation_status -eq 0 && $ic_confirm_status -eq 0 ]]; then
    echo "INCIDENT: IC activation failed; Base asset flows were re-paused and require operator review" >&2
  else
    echo "INCIDENT: activation compensation incomplete (ic_pause=$ic_pause_status timelock_cancel=$cancel_status operation_terminal=$operation_status deposit_pause=$deposit_pause_status withdrawal_pause=$withdrawal_pause_status base_confirm=$base_confirm_status ic_confirm=$ic_confirm_status); invoke the emergency runbook immediately" >&2
  fi
  return 1
}
ACTIVATION_STARTED=false
activation_failure() {
  local original_status=$?
  trap - ERR
  if [[ "$ACTIVATION_STARTED" == true ]]; then
    compensate_base_pause || true
  fi
  exit "$(( original_status == 0 ? 1 : original_status ))"
}
trap activation_failure ERR
ACTIVATION_STARTED=true
EXECUTE_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/bridge-execute.XXXXXX")"
cast send "$TIMELOCK" 'executeBatch(address[],uint256[],bytes[],bytes32,bytes32)' "$TARGETS" "$VALUES" "$PAYLOADS" "$ZERO" "$SALT" --rpc-url "$RPC" --ledger --from "$BRIDGE_BASE_ADMIN_ADDRESS" --json >"$EXECUTE_OUTPUT"
EXECUTE_TX="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1])).get("transactionHash",""))' "$EXECUTE_OUTPUT")"
rm -f "$EXECUTE_OUTPUT"
[[ "$EXECUTE_TX" =~ ^0x[0-9a-fA-F]{64}$ ]] || { echo "execute did not return a transaction hash" >&2; false; }
poll_canonical_activation false true "$EXECUTE_TX"
RESUME_OUTPUT="$(mktemp "${TMPDIR:-/tmp}/bridge-resume.XXXXXX")"
dfx canister call "$CANISTER" resume_new_deposits '()' --network "$IC_HOST" --identity "$BRIDGE_DFX_IDENTITY" --output json >"$RESUME_OUTPUT"
python3 -c 'import json,sys; v=json.load(open(sys.argv[1])); ok=[x for x in ([v] if not isinstance(v,dict) else [v]) if isinstance(x,dict) and "Ok" in x]; raise SystemExit(0 if len(ok)==1 else 1)' "$RESUME_OUTPUT"
rm -f "$RESUME_OUTPUT"
dfx canister call "$CANISTER" get_bridge_status '()' --network "$IC_HOST" --output json | python3 -c '
import json,sys
def values(v):
    if isinstance(v,dict): return ([v["deposits_paused"]] if "deposits_paused" in v else []) + sum((values(x) for x in v.values()),[])
    if isinstance(v,list): return sum((values(x) for x in v),[])
    return []
raise SystemExit(0 if values(json.load(sys.stdin)) == [False] else 1)'
ACTIVATION_STARTED=false
trap - ERR
