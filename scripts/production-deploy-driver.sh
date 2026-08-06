#!/usr/bin/env bash
# Fixed Gate A deployment sequence. It never enables asset acceptance.
set -euo pipefail
SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"
: "${BRIDGE_GATE_A_MANIFEST_SHA256:?missing Gate A approval}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_DEPLOYER_ADDRESS:?missing hardware-wallet deployer address}"
: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP CLI identity}"
: "${BRIDGE_DEPLOYMENT_BINDING_FILE:?missing deployment binding output path}"
: "${BRIDGE_DEPLOY_GAS_LIMIT:?missing reviewed deployment gas hard cap}"
: "${BRIDGE_DEPLOY_MAX_FEE_PER_GAS:?missing reviewed deployment max fee hard cap}"
: "${BRIDGE_DEPLOY_PRIORITY_FEE_PER_GAS:?missing reviewed deployment priority fee hard cap}"
[[ "$BRIDGE_DEPLOY_GAS_LIMIT" =~ ^[1-9][0-9]*$ && "$BRIDGE_DEPLOY_MAX_FEE_PER_GAS" =~ ^[1-9][0-9]*$ \
  && "$BRIDGE_DEPLOY_PRIORITY_FEE_PER_GAS" =~ ^[0-9]+$ \
  && "$BRIDGE_DEPLOY_PRIORITY_FEE_PER_GAS" -le "$BRIDGE_DEPLOY_MAX_FEE_PER_GAS" ]] || {
  echo "deployment gas caps are invalid" >&2; exit 1;
}
for tool in cast forge icp python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
production_validate_gate gate-a "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_A_MANIFEST_SHA256"
RENDERED_INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-deploy-inputs.XXXXXX")"
trap 'rm -rf "$RENDERED_INPUTS"' EXIT
production_render_release_inputs "$BRIDGE_RELEASE_BUNDLE" "$RENDERED_INPUTS"
CONSTRUCTOR_ARGS_FILE="$RENDERED_INPUTS/contract-constructor-args.json"

PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"; RPC="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["base_rpc_url"])' "$PROFILE")"
read -r CHAIN BRIDGE EXPECTED_TIMELOCK EXPECTED_TIMELOCK_CODE_HASH EXPECTED_BRIDGE_RUNTIME CANISTER EXPECTED_SIGNER GOVERNANCE_OPERATOR < <(python3 -c 'import json,sys;p=json.load(open(sys.argv[1]));print(p["chain_id"],p["bridge_contract"],p["timelock"]["address"],p["timelock"]["runtime_code_hash"],p["bridge_runtime_bytecode_sha256"],p["bridge_canister_id"],p["expected_bridge_signer"],p["governance_operator"])' "$PROFILE")
[[ "$(icp canister status bridge-canister -e production -i --identity "$BRIDGE_ICP_IDENTITY")" == "$CANISTER" ]] || {
  echo "production ICP environment does not map the reviewed Bridge Canister" >&2; exit 1;
}
PUBLIC_CONFIG_INITIALIZATION="$(icp canister call bridge-canister initialize_public_config '()' -e production --identity "$BRIDGE_ICP_IDENTITY" --json)"
python3 - "$PUBLIC_CONFIG_INITIALIZATION" <<'PY'
import json,sys
value=json.loads(sys.argv[1])
def values(node,key):
 out=[]
 if isinstance(node,dict):
  for k,v in node.items():
   if k==key: out.append(v)
   out.extend(values(v,key))
 elif isinstance(node,list):
  for item in node: out.extend(values(item,key))
 return out
if values(value,'Err') or len(values(value,'Ok')) != 1:
 raise SystemExit('Bridge public configuration initialization failed')
PY
PUBLIC_CONFIG="$(icp canister call bridge-canister get_public_config '()' -e production --query --json)"
STATUS="$(icp canister call bridge-canister get_bridge_status '()' -e production --json)"
python3 - "$EXPECTED_SIGNER" "$GOVERNANCE_OPERATOR" "$EXPECTED_BRIDGE_RUNTIME" "$PUBLIC_CONFIG" "$STATUS" <<'PY'
import json,sys
expected_signer,expected_operator,expected_runtime,public_raw,status_raw=sys.argv[1:]
def values(value,key):
 out=[]
 if isinstance(value,dict):
  for k,v in value.items():
   if k==key: out.append(v)
   out.extend(values(v,key))
 elif isinstance(value,list):
  for v in value: out.extend(values(v,key))
 return out
def address(value):
 return '0x'+bytes(value).hex() if isinstance(value,list) else str(value).lower()
public=json.loads(public_raw); status=json.loads(status_raw)
signers=values(public,'expected_bridge_signer'); operators=values(public,'governance_operator')
runtime_hashes=values(public,'expected_bridge_runtime_sha256')
if len(signers)!=1 or address(signers[0])!=expected_signer.lower(): raise SystemExit('production Canister Mint Signer differs from profile')
if len(operators)!=1 or address(operators[0])!=expected_operator.lower(): raise SystemExit('production Canister Governance Operator differs from profile')
if len(runtime_hashes)!=1 or bytes(runtime_hashes[0]).hex()!=expected_runtime.lower().removeprefix('0x'): raise SystemExit('production Canister expected Bridge runtime differs from profile')
if values(status,'deposits_paused') != [True]: raise SystemExit('production Canister must remain paused before Base deployment')
PY

checkpoint_stage() {
  local stage="$1" tx="$2" address="$3" receipt="$4"
  python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" "$stage" "$tx" "$address" "$receipt" <<'PY'
import json,os,re,sys
target,stage,tx,address,raw=sys.argv[1:]
r=json.loads(raw); number=lambda v:int(str(v),16) if str(v).startswith('0x') else int(v)
if not re.fullmatch(r'0x[0-9a-fA-F]{64}',tx) or number(r.get('status',0))!=1 or str(r.get('contractAddress','')).lower()!=address.lower():
 raise SystemExit('deployment receipt does not bind the created contract')
block_hash=str(r.get('blockHash','')).lower()
if not re.fullmatch(r'0x[0-9a-f]{64}',block_hash): raise SystemExit('deployment receipt block hash is invalid')
try:
 value=json.load(open(target,encoding='utf-8')) if os.path.getsize(target) else {}
except (FileNotFoundError,json.JSONDecodeError): value={}
value[stage]={'transaction_hash':tx.lower(),'address':address.lower(),'block_number':number(r['blockNumber']),'block_hash':block_hash}
tmp=target+'.tmp'; out=open(tmp,'w',encoding='utf-8'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
}
checkpoint_value() {
  python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" "$1" "$2" <<'PY'
import json,os,sys
p,key,field=sys.argv[1:]
if not os.path.exists(p) or not os.path.getsize(p): raise SystemExit(1)
print(json.load(open(p)).get(key,{}).get(field,''))
PY
}
predict_create_address() {
  local nonce="$1" output
  output="$(cast compute-address "$BRIDGE_DEPLOYER_ADDRESS" --nonce "$nonce")"
  python3 -c 'import re,sys;m=re.search(r"0x[0-9a-fA-F]{40}",sys.argv[1]);print(m.group(0) if m else (_ for _ in ()).throw(ValueError("cast did not return a CREATE address")))' "$output"
}
precheck_create() {
  local expected="$1" nonce predicted
  [[ "$(cast code "$expected" --rpc-url "$RPC")" == 0x ]] || { echo "expected CREATE address already has code" >&2; exit 1; }
  nonce="$(cast nonce "$BRIDGE_DEPLOYER_ADDRESS" --block pending --rpc-url "$RPC")"
  predicted="$(predict_create_address "$nonce")"
  [[ "$(printf '%s' "$predicted" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$expected" | tr '[:upper:]' '[:lower:]')" ]] || { echo "pending deployer nonce does not derive the reviewed CREATE address" >&2; exit 1; }
}
FORGE_CAPS=(--gas-limit "$BRIDGE_DEPLOY_GAS_LIMIT" --gas-price "$BRIDGE_DEPLOY_MAX_FEE_PER_GAS" --priority-gas-price "$BRIDGE_DEPLOY_PRIORITY_FEE_PER_GAS")

TIMELOCK_TX="$(checkpoint_value timelock transaction_hash 2>/dev/null || true)"
DEPLOYED_TIMELOCK="$(checkpoint_value timelock address 2>/dev/null || true)"
DEPLOYED_TIMELOCK="${DEPLOYED_TIMELOCK:-$EXPECTED_TIMELOCK}"
if [[ -z "$TIMELOCK_TX" ]]; then
  precheck_create "$EXPECTED_TIMELOCK"
  ARGS=(); while IFS= read -r x; do ARGS+=("$x"); done < <(python3 -c 'import json,sys; emit=lambda x: print("["+",".join(x)+"]" if isinstance(x,list) else x); [emit(x) for x in json.load(open(sys.argv[1]))["timelock"]]' "$CONSTRUCTOR_ARGS_FILE")
  TIMELOCK_JSON="$(FOUNDRY_PROFILE=default forge create --root "$SOURCE_ROOT/contracts" src/BridgeTimelockController.sol:BridgeTimelockController --broadcast --rpc-url "$RPC" --chain "$CHAIN" --ledger --from "$BRIDGE_DEPLOYER_ADDRESS" "${FORGE_CAPS[@]}" --json --constructor-args "${ARGS[@]}")"
  DEPLOYED_TIMELOCK="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["deployedTo"])' <<<"$TIMELOCK_JSON")"
  TIMELOCK_TX="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["transactionHash"])' <<<"$TIMELOCK_JSON")"
  TIMELOCK_RECEIPT="$(cast receipt "$TIMELOCK_TX" --rpc-url "$RPC" --json)"
  checkpoint_stage timelock "$TIMELOCK_TX" "$DEPLOYED_TIMELOCK" "$TIMELOCK_RECEIPT"
else
  TIMELOCK_RECEIPT="$(cast receipt "$TIMELOCK_TX" --rpc-url "$RPC" --json)"
fi
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
BRIDGE_TX="$(checkpoint_value bridge transaction_hash 2>/dev/null || true)"
DEPLOYED_BRIDGE="$(checkpoint_value bridge address 2>/dev/null || true)"
DEPLOYED_BRIDGE="${DEPLOYED_BRIDGE:-$BRIDGE}"
if [[ -z "$BRIDGE_TX" ]]; then
  precheck_create "$BRIDGE"
  BRIDGE_JSON="$(FOUNDRY_PROFILE=default forge create --root "$SOURCE_ROOT/contracts" src/Bridge.sol:Bridge --broadcast --rpc-url "$RPC" --chain "$CHAIN" --ledger --from "$BRIDGE_DEPLOYER_ADDRESS" "${FORGE_CAPS[@]}" --json --constructor-args "${ARGS[@]}")"
  DEPLOYED_BRIDGE="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["deployedTo"])' <<<"$BRIDGE_JSON")"
  BRIDGE_TX="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["transactionHash"])' <<<"$BRIDGE_JSON")"
  BRIDGE_RECEIPT="$(cast receipt "$BRIDGE_TX" --rpc-url "$RPC" --json)"
  checkpoint_stage bridge "$BRIDGE_TX" "$DEPLOYED_BRIDGE" "$BRIDGE_RECEIPT"
else
  BRIDGE_RECEIPT="$(cast receipt "$BRIDGE_TX" --rpc-url "$RPC" --json)"
fi
[[ "$(printf '%s' "$DEPLOYED_BRIDGE" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$BRIDGE" | tr '[:upper:]' '[:lower:]')" ]] || { echo "Bridge address mismatch" >&2; exit 1; }
[[ "$(cast call "$BRIDGE" 'depositMintsPaused()(bool)' --rpc-url "$RPC")" == true && "$(cast call "$BRIDGE" 'withdrawalsPaused()(bool)' --rpc-url "$RPC")" == true ]] || { echo "new Bridge is not paused" >&2; exit 1; }
BRIDGE_RUNTIME_CODE="$(cast code "$BRIDGE" --rpc-url "$RPC")"
BRIDGE_RUNTIME_SHA256="$(python3 -c 'import hashlib,sys;v=sys.argv[1];print(hashlib.sha256(bytes.fromhex(v.removeprefix("0x"))).hexdigest())' "$BRIDGE_RUNTIME_CODE")"
[[ "$(printf '%s' "$BRIDGE_RUNTIME_SHA256" | tr '[:upper:]' '[:lower:]')" == "$(printf '%s' "$EXPECTED_BRIDGE_RUNTIME" | tr '[:upper:]' '[:lower:]')" ]] || { echo "deployed Bridge runtime bytecode hash mismatch" >&2; exit 1; }
python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" "$TIMELOCK_TX" "$DEPLOYED_TIMELOCK" "$TIMELOCK_RECEIPT" "$BRIDGE_TX" "$DEPLOYED_BRIDGE" "$BRIDGE_RECEIPT" <<'PY'
import json,os,re,sys
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
with open(target+'.tmp','w',encoding='utf-8') as out:
 json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno())
os.replace(target+'.tmp',target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
