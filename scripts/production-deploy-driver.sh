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
for tool in cast forge dfx python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
production_validate_gate gate-a "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_A_MANIFEST_SHA256"
RENDERED_INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-deploy-inputs.XXXXXX")"
trap 'rm -rf "$RENDERED_INPUTS"' EXIT
production_render_release_inputs "$BRIDGE_RELEASE_BUNDLE" "$RENDERED_INPUTS"
CANISTER_INIT_FILE="$RENDERED_INPUTS/canister-init.json"
CONSTRUCTOR_ARGS_FILE="$RENDERED_INPUTS/contract-constructor-args.json"

PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"; RPC="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["base_rpc_url"])' "$PROFILE")"
read -r CHAIN BRIDGE EXPECTED_TIMELOCK CANISTER IC_HOST < <(python3 -c 'import json,sys;p=json.load(open(sys.argv[1]));print(p["chain_id"],p["bridge_contract"],p["timelock"]["address"],p["bridge_canister_id"],p["ic_host"])' "$PROFILE")
ARGS=(); while IFS= read -r x; do ARGS+=("$x"); done < <(python3 -c 'import json,sys; emit=lambda x: print("["+",".join(x)+"]" if isinstance(x,list) else x); [emit(x) for x in json.load(open(sys.argv[1]))["timelock"]]' "$CONSTRUCTOR_ARGS_FILE")
TIMELOCK_JSON="$(forge create --root "$SOURCE_ROOT/contracts" src/BridgeTimelockController.sol:BridgeTimelockController --broadcast --rpc-url "$RPC" --chain "$CHAIN" --ledger --from "$BRIDGE_DEPLOYER_ADDRESS" --json --constructor-args "${ARGS[@]}")"
DEPLOYED_TIMELOCK="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["deployedTo"])' <<<"$TIMELOCK_JSON")"
[[ "$(printf '%s' "$DEPLOYED_TIMELOCK" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$EXPECTED_TIMELOCK" | tr '[:upper:]' '[:lower:]')" ]] || { echo "Timelock address mismatch" >&2; exit 1; }
ARGS=(); while IFS= read -r x; do ARGS+=("$x"); done < <(python3 -c 'import json,sys;[print(x) for x in json.load(open(sys.argv[1]))["bridge"]]' "$CONSTRUCTOR_ARGS_FILE")
BRIDGE_JSON="$(forge create --root "$SOURCE_ROOT/contracts" src/Bridge.sol:Bridge --broadcast --rpc-url "$RPC" --chain "$CHAIN" --ledger --from "$BRIDGE_DEPLOYER_ADDRESS" --json --constructor-args "${ARGS[@]}")"
DEPLOYED_BRIDGE="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["deployedTo"])' <<<"$BRIDGE_JSON")"
[[ "$(printf '%s' "$DEPLOYED_BRIDGE" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$BRIDGE" | tr '[:upper:]' '[:lower:]')" ]] || { echo "Bridge address mismatch" >&2; exit 1; }
[[ "$(cast call "$BRIDGE" 'depositMintsPaused()(bool)' --rpc-url "$RPC")" == true && "$(cast call "$BRIDGE" 'withdrawalsPaused()(bool)' --rpc-url "$RPC")" == true ]] || { echo "new Bridge is not paused" >&2; exit 1; }

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
