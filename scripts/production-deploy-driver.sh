#!/usr/bin/env bash
# Fixed Gate A deployment sequence. It never enables asset acceptance.
set -euo pipefail
SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"
: "${BRIDGE_GATE_A_MANIFEST_SHA256:?missing Gate A approval}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_DEPLOYER_ADDRESS:?missing hardware-wallet deployer address}"
: "${BRIDGE_DFX_IDENTITY:?missing reviewed dfx identity}"
: "${BRIDGE_DEPLOYMENT_BINDING_FILE:?missing deployment binding output path}"
for tool in cast forge dfx python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
production_validate_gate gate-a "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_A_MANIFEST_SHA256"
RENDERED_INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-deploy-inputs.XXXXXX")"
trap 'rm -rf "$RENDERED_INPUTS"' EXIT
production_render_release_inputs "$BRIDGE_RELEASE_BUNDLE" "$RENDERED_INPUTS"
CANISTER_INIT_FILE="$RENDERED_INPUTS/canister-init.json"
CONSTRUCTOR_ARGS_FILE="$RENDERED_INPUTS/contract-constructor-args.json"

PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"; RPC="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["base_rpc_url"])' "$PROFILE")"
read -r CHAIN BRIDGE EXPECTED_TIMELOCK EXPECTED_TIMELOCK_CODE_HASH CANISTER IC_HOST < <(python3 -c 'import json,sys;p=json.load(open(sys.argv[1]));print(p["chain_id"],p["bridge_contract"],p["timelock"]["address"],p["timelock"]["runtime_code_hash"],p["bridge_canister_id"],p["ic_host"])' "$PROFILE")
ARGS=(); while IFS= read -r x; do ARGS+=("$x"); done < <(python3 -c 'import json,sys; emit=lambda x: print("["+",".join(x)+"]" if isinstance(x,list) else x); [emit(x) for x in json.load(open(sys.argv[1]))["timelock"]]' "$CONSTRUCTOR_ARGS_FILE")
TIMELOCK_JSON="$(forge create --root "$SOURCE_ROOT/contracts" src/BridgeTimelockController.sol:BridgeTimelockController --broadcast --rpc-url "$RPC" --chain "$CHAIN" --ledger --from "$BRIDGE_DEPLOYER_ADDRESS" --json --constructor-args "${ARGS[@]}")"
DEPLOYED_TIMELOCK="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["deployedTo"])' <<<"$TIMELOCK_JSON")"
TIMELOCK_TX="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["transactionHash"])' <<<"$TIMELOCK_JSON")"
[[ "$(printf '%s' "$DEPLOYED_TIMELOCK" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$EXPECTED_TIMELOCK" | tr '[:upper:]' '[:lower:]')" ]] || { echo "Timelock address mismatch" >&2; exit 1; }
TIMELOCK_RUNTIME_CODE="$(cast code "$DEPLOYED_TIMELOCK" --rpc-url "$RPC")"
DEPLOYED_TIMELOCK_CODE_HASH="$(cast keccak "$TIMELOCK_RUNTIME_CODE")"
ARGS=(); while IFS= read -r x; do ARGS+=("$x"); done < <(python3 -c 'import json,sys;[print(x) for x in json.load(open(sys.argv[1]))["bridge"]]' "$CONSTRUCTOR_ARGS_FILE")
[[ ${#ARGS[@]} -ge 7 ]] || { echo "rendered Bridge constructor args are incomplete" >&2; exit 1; }
RENDERED_TIMELOCK_CODE_HASH="$(printf '%s' "${ARGS[6]}" | tr '[:upper:]' '[:lower:]')"
EXPECTED_TIMELOCK_CODE_HASH_LOWER="$(printf '%s' "$EXPECTED_TIMELOCK_CODE_HASH" | tr '[:upper:]' '[:lower:]')"
DEPLOYED_TIMELOCK_CODE_HASH_LOWER="$(printf '%s' "$DEPLOYED_TIMELOCK_CODE_HASH" | tr '[:upper:]' '[:lower:]')"
[[ "$RENDERED_TIMELOCK_CODE_HASH" == "$EXPECTED_TIMELOCK_CODE_HASH_LOWER" ]] || { echo "rendered Timelock code hash differs from profile" >&2; exit 1; }
[[ "$DEPLOYED_TIMELOCK_CODE_HASH_LOWER" == "$EXPECTED_TIMELOCK_CODE_HASH_LOWER" ]] || { echo "deployed Timelock runtime code hash mismatch" >&2; exit 1; }
BRIDGE_JSON="$(forge create --root "$SOURCE_ROOT/contracts" src/Bridge.sol:Bridge --broadcast --rpc-url "$RPC" --chain "$CHAIN" --ledger --from "$BRIDGE_DEPLOYER_ADDRESS" --json --constructor-args "${ARGS[@]}")"
DEPLOYED_BRIDGE="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["deployedTo"])' <<<"$BRIDGE_JSON")"
BRIDGE_TX="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["transactionHash"])' <<<"$BRIDGE_JSON")"
[[ "$(printf '%s' "$DEPLOYED_BRIDGE" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$BRIDGE" | tr '[:upper:]' '[:lower:]')" ]] || { echo "Bridge address mismatch" >&2; exit 1; }
[[ "$(cast call "$BRIDGE" 'depositMintsPaused()(bool)' --rpc-url "$RPC")" == true && "$(cast call "$BRIDGE" 'withdrawalsPaused()(bool)' --rpc-url "$RPC")" == true ]] || { echo "new Bridge is not paused" >&2; exit 1; }
TIMELOCK_RECEIPT="$(cast receipt "$TIMELOCK_TX" --rpc-url "$RPC" --json)"
BRIDGE_RECEIPT="$(cast receipt "$BRIDGE_TX" --rpc-url "$RPC" --json)"
python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" "$TIMELOCK_TX" "$DEPLOYED_TIMELOCK" "$TIMELOCK_RECEIPT" "$BRIDGE_TX" "$DEPLOYED_BRIDGE" "$BRIDGE_RECEIPT" <<'PY'
import json,re,sys
target,tl_tx,tl_address,tl_raw,bridge_tx,bridge_address,bridge_raw=sys.argv[1:]
def parse(tx,address,raw):
 r=json.loads(raw); number=lambda value:int(str(value),16) if str(value).startswith('0x') else int(value)
 if not re.fullmatch(r'0x[0-9a-fA-F]{64}',tx) or number(r.get('status',0))!=1 or str(r.get('contractAddress','')).lower()!=address.lower():
  raise SystemExit('deployment receipt does not bind the created contract')
 block_hash=str(r.get('blockHash','')).lower()
 if not re.fullmatch(r'0x[0-9a-f]{64}',block_hash): raise SystemExit('deployment receipt block hash is invalid')
 return {'transaction_hash':tx.lower(),'block_number':number(r['blockNumber']),'block_hash':block_hash}
value={'timelock':parse(tl_tx,tl_address,tl_raw),'bridge':parse(bridge_tx,bridge_address,bridge_raw)}
if value['timelock']['block_number']>value['bridge']['block_number']: raise SystemExit('Timelock must be deployed before Bridge')
with open(target+'.tmp','w',encoding='utf-8') as out: json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n')
import os; os.replace(target+'.tmp',target)
PY

INIT="$(mktemp "${TMPDIR:-/tmp}/bridge-init.XXXXXX")"; trap 'rm -f "$INIT"' EXIT
trap 'rm -rf "$RENDERED_INPUTS"; rm -f "$INIT"' EXIT
python3 - "$CANISTER_INIT_FILE" >"$INIT" <<'PY'
import json,sys
p=json.load(open(sys.argv[1])); q=lambda s:'"'+str(s).replace('\\','\\\\').replace('"','\\"')+'"'
blob=lambda h:'blob "'+''.join('\\'+h[i:i+2] for i in range(0,len(h),2))+'"'
fields={
'settlement_rate_limit_global':p['settlement_rate_limit_global'],'settlement_rate_limit_per_principal':p['settlement_rate_limit_per_principal'],'settlement_cycle_ceiling':p['settlement_cycle_ceiling'],'settlement_rate_limit_per_record':p['settlement_rate_limit_per_record'],'finance_administrator':'principal '+q(p['finance_administrator']),'deposit_rate_limit_window_seconds':p['deposit_rate_limit_window_seconds'],'ecdsa_key_name':q(p['ecdsa_key_name']),'base_chain_id':p['base_chain_id'],'bridge_contract':blob(p['bridge_contract_hex']),'max_priority_fee_per_gas':p['max_priority_fee_per_gas'],'fee_recipient':'record { owner = principal '+q(p['fee_recipient']['owner'])+'; subaccount = '+blob(p['fee_recipient']['subaccount_hex'])+' }','settlement_rate_limit_window_seconds':p['settlement_rate_limit_window_seconds'],'ecdsa_derivation_path':'vec {'+';'.join(blob(x.encode().hex()) for x in p['ecdsa_derivation_path_utf8'])+'}','evm_rpc_canister_id':'principal '+q(p['evm_rpc_canister_id']),'deposit_rate_limit_per_principal':p['deposit_rate_limit_per_principal'],'pause_principals':'vec {'+';'.join('principal '+q(x) for x in p['pause_principals'])+'}','max_fee_per_gas':p['max_fee_per_gas'],'eth_floor_wei':p['eth_floor_wei'],'custom_evm_rpc_urls':'vec {'+';'.join(q(x) for x in p['custom_evm_rpc_urls'])+'}','transaction_gas_limit':p['transaction_gas_limit'],'deposit_rate_limit_global':p['deposit_rate_limit_global'],'governance_principal':'principal '+q(p['governance_principal']),'index_canister_id':'principal '+q(p['index_canister_id']),'ledger_canister_id':'principal '+q(p['ledger_canister_id']),'cycles_floor':p['cycles_floor']}
print('(record {'+';'.join(f'{k} = {v}' for k,v in fields.items())+'})')
PY
dfx canister install "$CANISTER" --network "$IC_HOST" --identity "$BRIDGE_DFX_IDENTITY" --mode install --wasm "$BRIDGE_RELEASE_BUNDLE/bridge-canister.wasm" --argument "$(cat "$INIT")"
dfx canister call "$CANISTER" get_bridge_status '()' --network "$IC_HOST" --output json | python3 -c '
import json,sys
def values(v):
    if isinstance(v,dict):
        return ([v["deposits_paused"]] if "deposits_paused" in v else []) + sum((values(x) for x in v.values()),[])
    if isinstance(v,list): return sum((values(x) for x in v),[])
    return []
found=values(json.load(sys.stdin))
raise SystemExit(0 if found == [True] else 1)'
