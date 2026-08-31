#!/usr/bin/env bash
# Gate A contract placement by a fresh, roleless EOA. This script never activates assets.
set -euo pipefail
SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_A_MANIFEST_SHA256:?missing Gate A approval}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_CANISTER_INSTALL_RECEIPT:?missing verified production Canister install receipt}"
: "${BRIDGE_DEPLOYER_KEYSTORE:?missing encrypted Foundry keystore path}"
: "${BRIDGE_DEPLOYER_PASSWORD_FILE:?missing separate password-file path}"
: "${BRIDGE_DEPLOYMENT_BINDING_FILE:?missing deployment evidence output path}"
: "${BRIDGE_DEPLOYMENT_RESERVATION_FILE:?missing deployment reservation path}"
: "${BASE_RPC_URL:?missing Base transaction transport URL}"
for tool in awk cast forge python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
[[ -f "$BRIDGE_DEPLOYER_KEYSTORE" && -f "$BRIDGE_DEPLOYER_PASSWORD_FILE" ]] || { echo "keystore or password file is missing" >&2; exit 1; }
[[ "$BRIDGE_DEPLOYMENT_RESERVATION_FILE" == "$BRIDGE_DEPLOYMENT_BINDING_FILE.reservation" \
  && -f "$BRIDGE_DEPLOYMENT_RESERVATION_FILE" \
  && ! -L "$BRIDGE_DEPLOYMENT_RESERVATION_FILE" \
  && ! -s "$BRIDGE_DEPLOYMENT_RESERVATION_FILE" ]] || {
  echo "deployment reservation is missing, non-empty, or not the canonical marker" >&2; exit 1;
}
[[ ! -e "$BRIDGE_DEPLOYMENT_BINDING_FILE" && ! -e "$BRIDGE_DEPLOYMENT_BINDING_FILE.checkpoint" ]] || {
  echo "deployment evidence/checkpoint already exists; track it instead of redeploying" >&2; exit 1;
}

production_validate_gate gate-a "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_A_MANIFEST_SHA256" \
  "$BRIDGE_CANISTER_INSTALL_RECEIPT"
INPUTS="$(mktemp -d "${TMPDIR:-/tmp}/bridge-eoa-deploy.XXXXXX")"; trap 'rm -rf "$INPUTS"' EXIT
production_render_release_inputs "$BRIDGE_RELEASE_BUNDLE" "$INPUTS"
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"; CONSTRUCTORS="$INPUTS/contract-constructor-args.json"
read -r CHAIN DEPLOYER START_NONCE BRIDGE TIMELOCK GAS_LIMIT MAX_FEE PRIORITY_FEE < <(python3 -c '
import json,sys
p=json.load(open(sys.argv[1]));d=p["initial_base_deployment"]
print(p["chain_id"],d["deployer_address"],d["starting_nonce"],p["bridge_contract"],p["timelock"]["address"],d["gas_limit"],d["max_fee_per_gas"],d["max_priority_fee_per_gas"])
' "$PROFILE")

[[ "$(cast chain-id --rpc-url "$BASE_RPC_URL")" == "$CHAIN" && "$CHAIN" == 8453 ]] || { echo "Base chain mismatch" >&2; exit 1; }
KEYSTORE_ADDRESS="$(cast wallet address --keystore "$BRIDGE_DEPLOYER_KEYSTORE" --password-file "$BRIDGE_DEPLOYER_PASSWORD_FILE")"
lower() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }
[[ "$(lower "$KEYSTORE_ADDRESS")" == "$(lower "$DEPLOYER")" ]] || { echo "keystore address mismatch" >&2; exit 1; }
PENDING_NONCE="$(cast nonce "$DEPLOYER" --block pending --rpc-url "$BASE_RPC_URL")"
[[ "$PENDING_NONCE" == "$START_NONCE" ]] || { echo "nonce drift; approve a new profile" >&2; exit 1; }
BRIDGE_NONCE="$((START_NONCE + 1))"
[[ "$(cast compute-address "$DEPLOYER" --nonce "$START_NONCE" | awk '{print tolower($NF)}')" == "$(lower "$TIMELOCK")" ]] || { echo "Timelock CREATE mismatch" >&2; exit 1; }
[[ "$(cast compute-address "$DEPLOYER" --nonce "$BRIDGE_NONCE" | awk '{print tolower($NF)}')" == "$(lower "$BRIDGE")" ]] || { echo "Bridge CREATE mismatch" >&2; exit 1; }
[[ "$(cast code "$TIMELOCK" --rpc-url "$BASE_RPC_URL")" == 0x && "$(cast code "$BRIDGE" --rpc-url "$BASE_RPC_URL")" == 0x ]] || { echo "CREATE address already has code" >&2; exit 1; }
BALANCE="$(cast balance "$DEPLOYER" --rpc-url "$BASE_RPC_URL")"
LIABILITY="$(python3 -c 'import sys;print(int(sys.argv[1])*int(sys.argv[2])*2)' "$GAS_LIMIT" "$MAX_FEE")"
python3 -c 'import sys;sys.exit("insufficient deployer balance") if int(sys.argv[1]) <= int(sys.argv[2]) else None' "$BALANCE" "$LIABILITY"
PENDING_BLOCK="$(cast block pending --rpc-url "$BASE_RPC_URL" --json)" || { echo "current Base fee unavailable" >&2; exit 1; }
SUGGESTED_PRIORITY_FEE="$(cast rpc --rpc-url "$BASE_RPC_URL" eth_maxPriorityFeePerGas)" || { echo "current Base priority fee unavailable" >&2; exit 1; }
python3 - "$PENDING_BLOCK" "$SUGGESTED_PRIORITY_FEE" "$MAX_FEE" "$PRIORITY_FEE" <<'PY'
import json,re,sys

def rpc_quantity(value, name):
    if not isinstance(value, str) or not re.fullmatch(r'0x(?:0|[1-9a-fA-F][0-9a-fA-F]*)', value):
        raise SystemExit(f'{name} is malformed')
    return int(value, 16)

def profile_quantity(value, name):
    if not isinstance(value, str) or not re.fullmatch(r'(?:0|[1-9][0-9]*)', value):
        raise SystemExit(f'{name} is malformed')
    return int(value)

try:
    pending = json.loads(sys.argv[1])
    suggested = json.loads(sys.argv[2])
except json.JSONDecodeError:
    raise SystemExit('current Base fee response is malformed')
base_fee = rpc_quantity(pending.get('baseFeePerGas'), 'current Base fee') if isinstance(pending, dict) else rpc_quantity(None, 'current Base fee')
suggested_priority = rpc_quantity(suggested, 'current Base priority fee')
max_fee = profile_quantity(sys.argv[3], 'profile max fee')
priority_fee = profile_quantity(sys.argv[4], 'profile priority fee')
if suggested_priority > priority_fee:
    raise SystemExit('current Base priority fee exceeds the approved profile ceiling; no transaction submitted')
if base_fee + priority_fee > max_fee:
    raise SystemExit('current Base fee exceeds the approved profile ceiling; no transaction submitted')
PY

checkpoint() {
  python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE.checkpoint" "$1" "$2" "$3" "${4:-}" <<'PY'
import json,os,re,sys
target,phase,address,nonce,transaction_hash=sys.argv[1:];tmp=target+'.tmp'
value={'phase':phase,'expected_address':address.lower(),'nonce':int(nonce)}
if transaction_hash:
 if not re.fullmatch(r'0x[0-9a-fA-F]{64}',transaction_hash): raise SystemExit('submission transaction hash is invalid')
 value['transaction_hash']=transaction_hash.lower()
with open(tmp,'w',encoding='utf-8') as out:
 json.dump(value,out,sort_keys=True,separators=(',',':'));out.write('\n');out.flush();os.fsync(out.fileno())
os.replace(tmp,target)
parent=os.open(os.path.dirname(os.path.abspath(target)),os.O_RDONLY|getattr(os,'O_DIRECTORY',0))
try: os.fsync(parent)
finally: os.close(parent)
PY
}

deploy() {
  local contract="$1" expected="$2" nonce="$3" args_key="$4" json tx receipt; local args=()
  while IFS= read -r value; do args+=("$value"); done < <(python3 - "$CONSTRUCTORS" "$args_key" <<'PY'
import json,sys
for value in json.load(open(sys.argv[1]))[sys.argv[2]]: print('['+','.join(value)+']' if isinstance(value,list) else value)
PY
  )
  checkpoint "${args_key}_submission_started" "$expected" "$nonce"
  json="$(FOUNDRY_PROFILE=default forge create --root "$SOURCE_ROOT/contracts" "$contract" --broadcast --rpc-url "$BASE_RPC_URL" --chain "$CHAIN" --nonce "$nonce" --keystore "$BRIDGE_DEPLOYER_KEYSTORE" --password-file "$BRIDGE_DEPLOYER_PASSWORD_FILE" --from "$DEPLOYER" --gas-limit "$GAS_LIMIT" --gas-price "$MAX_FEE" --priority-gas-price "$PRIORITY_FEE" --json --constructor-args "${args[@]}")" || { echo "submission result unknown; inspect checkpoint and do not rerun" >&2; exit 1; }
  tx="$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["transactionHash"])' <<<"$json")"
  checkpoint "${args_key}_submitted" "$expected" "$nonce" "$tx"
  receipt="$(cast receipt "$tx" --rpc-url "$BASE_RPC_URL" --json)" || { echo "track $tx; do not redeploy" >&2; exit 1; }
  if ! python3 - "$tx" "$expected" "$receipt" <<'PY'
import json,re,sys
tx,address,raw=sys.argv[1:];r=json.loads(raw);n=lambda v:int(str(v),16) if str(v).startswith('0x') else int(v)
if not re.fullmatch(r'0x[0-9a-fA-F]{64}',tx) or n(r.get('status',0))!=1: raise SystemExit('deployment reverted')
if r.get('contractAddress','').lower()!=address.lower(): raise SystemExit('receipt CREATE address mismatch')
if not re.fullmatch(r'0x[0-9a-fA-F]{64}',str(r.get('blockHash',''))): raise SystemExit('receipt block hash is invalid')
PY
  then
    return 1
  fi
  printf '%s\t%s\t%s\n' "$tx" "$(python3 -c 'import json,sys;v=json.loads(sys.stdin.read())["blockNumber"];print(int(v,16) if str(v).startswith("0x") else int(v))' <<<"$receipt")" "$(python3 -c 'import json,sys;print(json.loads(sys.stdin.read())["blockHash"].lower())' <<<"$receipt")"
}

TIMELOCK_RESULT="$(deploy src/BridgeTimelockController.sol:BridgeTimelockController "$TIMELOCK" "$START_NONCE" timelock)"
checkpoint timelock_finalized "$TIMELOCK" "$START_NONCE"
BRIDGE_RESULT="$(deploy src/Bridge.sol:Bridge "$BRIDGE" "$BRIDGE_NONCE" bridge)"
python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" "$TIMELOCK_RESULT" "$TIMELOCK" "$BRIDGE_RESULT" "$BRIDGE" "$DEPLOYER" "$START_NONCE" <<'PY'
import json,os,sys
target,tl,tla,b,ba,deployer,nonce=sys.argv[1:];tt,tb,tbh=tl.split('\t');bt,bb,bbh=b.split('\t')
v={'deployer_address':deployer.lower(),'starting_nonce':int(nonce),'timelock':{'transaction_hash':tt.lower(),'address':tla.lower(),'block_number':int(tb),'block_hash':tbh},'bridge':{'transaction_hash':bt.lower(),'address':ba.lower(),'block_number':int(bb),'block_hash':bbh}}
if v['timelock']['block_number']>v['bridge']['block_number']: raise SystemExit('deployment order mismatch')
tmp=target+'.tmp'
with open(tmp,'w',encoding='utf-8') as out: json.dump(v,out,sort_keys=True,separators=(',',':'));out.write('\n');out.flush();os.fsync(out.fileno())
os.replace(tmp,target)
parent=os.open(os.path.dirname(os.path.abspath(target)),os.O_RDONLY|getattr(os,'O_DIRECTORY',0))
try: os.fsync(parent)
finally: os.close(parent)
PY
python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" <<'PY'
import os,sys
parent=os.open(os.path.dirname(os.path.abspath(sys.argv[1])),os.O_RDONLY|getattr(os,'O_DIRECTORY',0))
try: os.fsync(parent)
finally: os.close(parent)
PY
rm "$BRIDGE_DEPLOYMENT_BINDING_FILE.checkpoint"
rm "$BRIDGE_DEPLOYMENT_RESERVATION_FILE"
python3 - "$BRIDGE_DEPLOYMENT_BINDING_FILE" <<'PY'
import os,sys
parent=os.open(os.path.dirname(os.path.abspath(sys.argv[1])),os.O_RDONLY|getattr(os,'O_DIRECTORY',0))
try: os.fsync(parent)
finally: os.close(parent)
PY
echo "Contracts deployed paused. Verify the Canister audit, then sweep residual ETH and record the sweep transaction."
