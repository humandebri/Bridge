#!/usr/bin/env bash
# Capture live state and a release-bound chain-key control signature from the reviewed profile.
set -euo pipefail

MODE="${1:-verify}"
if [[ "$MODE" == capture ]]; then
  BUNDLE="${2:?usage: production-live-preflight.sh capture BUNDLE OUTPUT}"
  OUTPUT="${3:?usage: production-live-preflight.sh capture BUNDLE OUTPUT}"
elif [[ "$MODE" == verify ]]; then
  BUNDLE="${2:?usage: production-live-preflight.sh verify BUNDLE}"
  OUTPUT=""
else
  echo "usage: production-live-preflight.sh {capture BUNDLE OUTPUT|verify BUNDLE}" >&2; exit 2
fi
PROFILE="$BUNDLE/profile.json"
SNAPSHOT="$BUNDLE/signer-snapshot.json"
MANIFEST="$BUNDLE/release-manifest.json"
[[ -f "$PROFILE" && -f "$MANIFEST" ]] || { echo "profile or release manifest is missing" >&2; exit 1; }
if [[ "$MODE" == verify && ! -f "$SNAPSHOT" ]]; then
  echo "signed snapshot is missing" >&2; exit 1
fi
: "${BRIDGE_DFX_IDENTITY:?BRIDGE_DFX_IDENTITY must name the governance identity used for the chain-key challenge}"
for tool in cast dfx python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

PROFILE_VALUES=()
while IFS= read -r line; do PROFILE_VALUES[${#PROFILE_VALUES[@]}]="$line"; done < <(python3 - "$PROFILE" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]))
for v in [p['chain_id'],p['evm_rpc_canister_id'],p['bridge_canister_id'],p['bridge_contract'],p['timelock']['address'],p['root_canister_id'],p['bridge_runtime_bytecode_sha256'],p['bridge_canister_wasm_sha256'],p['ic_host'],*map(lambda x:x['url'],p['rpc_providers'])]: print(v)
PY
)
[[ ${#PROFILE_VALUES[@]} -eq 12 ]] || { echo "profile must contain exactly three RPC providers" >&2; exit 1; }
CHAIN_ID=${PROFILE_VALUES[0]}; EVM_RPC_CANISTER=${PROFILE_VALUES[1]}; CANISTER=${PROFILE_VALUES[2]}
BRIDGE=${PROFILE_VALUES[3]}; TIMELOCK=${PROFILE_VALUES[4]}; EXPECTED_CONTROLLER=${PROFILE_VALUES[5]}
EXPECTED_RUNTIME=${PROFILE_VALUES[6]}; EXPECTED_WASM=${PROFILE_VALUES[7]}; IC_HOST=${PROFILE_VALUES[8]}
RELEASE_ID="$(python3 - "$MANIFEST" <<'PY'
import json,re,sys
value=json.load(open(sys.argv[1]))['release_id']
if not isinstance(value,str) or not re.fullmatch(r'[a-z0-9-]{8,64}',value): raise SystemExit('invalid release_id')
print(value)
PY
)"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-live-preflight.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
for i in 0 1 2; do
  rpc="${PROFILE_VALUES[$((9+i))]}"
  [[ "$(cast chain-id --rpc-url "$rpc")" == "$CHAIN_ID" ]] || { echo "RPC chain id mismatch" >&2; exit 1; }
  cast block safe --rpc-url "$rpc" --json >"$TMP/block-$i.json"
  if [[ "$MODE" == verify ]]; then
    SIGNED_HEIGHT="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["confirmed_head_block_number"])' "$SNAPSHOT")"
    cast block "$SIGNED_HEIGHT" --rpc-url "$rpc" --json >"$TMP/signed-block-$i.json"
  fi
done

python3 - "$PROFILE" "$TMP" <<'PY'
import hashlib,json,subprocess,sys
from pathlib import Path
p=json.load(open(sys.argv[1])); root=Path(sys.argv[2])
blocks=[]
for i in range(3):
 b=json.load(open(root/f'block-{i}.json')); n=int(str(b['number']),16) if str(b['number']).startswith('0x') else int(b['number']); blocks.append((n,str(b['hash']).lower()))
height,block_hash=max(set(blocks),key=blocks.count)
if blocks.count((height,block_hash))<2: raise SystemExit('no Safe block quorum')
roles={name:subprocess.check_output(['cast','keccak',name],text=True).strip() for name in ('PROPOSER_ROLE','EXECUTOR_ROLE','CANCELLER_ROLE')}
zero='0x'+'00'*20; zero32='0x'+'00'*32
def run(args): return subprocess.check_output(args,text=True).strip().strip('"')
def call(rpc,address,sig,*args): return run(['cast','call',address,sig,*args,'--rpc-url',rpc,'--block',str(height)])
def code(rpc,address): return run(['cast','code',address,'--rpc-url',rpc,'--block',str(height)])
def sha_code(value): return hashlib.sha256(bytes.fromhex(value.removeprefix('0x'))).hexdigest()
states=[]
for rpc_entry in p['rpc_providers']:
 rpc=rpc_entry['url']; bridge=p['bridge_contract']; timelock=p['timelock']['address']
 bsns=call(rpc,bridge,'bsns()(address)').lower()
 external={p['timelock']['proposer'],p['timelock']['executor'],p['timelock']['canceller'],p['base_admin_wallet'],p['runtime_administrator'],p['release_approver'],zero}
 state={
  'height':height,'hash':block_hash,
  'base_bridge_signer':call(rpc,bridge,'bridgeSigner()(address)').lower(),
  'base_runtime_administrator':call(rpc,bridge,'runtimeAdministrator()(address)').lower(),
  'base_admin_timelock':call(rpc,bridge,'baseAdminTimelock()(address)').lower(),
  'base_deposit_mints_paused':call(rpc,bridge,'depositMintsPaused()(bool)').lower()=='true',
  'base_withdrawals_paused':call(rpc,bridge,'withdrawalsPaused()(bool)').lower()=='true',
  'bridge_runtime_bytecode_sha256':sha_code(code(rpc,bridge)),
  'timelock_minimum_delay_seconds':int(call(rpc,timelock,'getMinDelay()(uint256)').split()[0],0),
  'timelock_self_admin':call(rpc,timelock,'hasRole(bytes32,address)(bool)',zero32,timelock).lower()=='true',
  'timelock_proposer_authorized':call(rpc,timelock,'hasRole(bytes32,address)(bool)',roles['PROPOSER_ROLE'],p['timelock']['proposer']).lower()=='true',
  'timelock_executor_authorized':call(rpc,timelock,'hasRole(bytes32,address)(bool)',roles['EXECUTOR_ROLE'],p['timelock']['executor']).lower()=='true',
  'timelock_canceller_authorized':call(rpc,timelock,'hasRole(bytes32,address)(bool)',roles['CANCELLER_ROLE'],p['timelock']['canceller']).lower()=='true',
  'timelock_open_proposer':call(rpc,timelock,'hasRole(bytes32,address)(bool)',roles['PROPOSER_ROLE'],zero).lower()=='true',
  'timelock_open_executor':call(rpc,timelock,'hasRole(bytes32,address)(bool)',roles['EXECUTOR_ROLE'],zero).lower()=='true',
  'timelock_open_canceller':call(rpc,timelock,'hasRole(bytes32,address)(bool)',roles['CANCELLER_ROLE'],zero).lower()=='true',
  'bsns_address':bsns,'bsns_runtime_bytecode_sha256':sha_code(code(rpc,bsns)),
  'bsns_name':call(rpc,bsns,'name()(string)'),'bsns_symbol':call(rpc,bsns,'symbol()(string)'),
  'bsns_decimals':int(call(rpc,bsns,'decimals()(uint8)').split()[0],0),'bsns_bridge':call(rpc,bsns,'bridge()(address)').lower(),
 }
 state['timelock_external_admins_absent']=all(call(rpc,timelock,'hasRole(bytes32,address)(bool)',zero32,a).lower()=='false' for a in external if a.lower()!=timelock.lower())
 states.append(state)
groups={json.dumps(s,sort_keys=True,separators=(',',':')):states.count(s) for s in states}
winner,count=max(groups.items(),key=lambda x:x[1])
if count<2: raise SystemExit('Base state does not have 2-of-3 agreement at the Safe block')
(root/'base-state.json').write_text(json.dumps({'agreeing_providers':count,'state':json.loads(winner)},sort_keys=True,separators=(',',':'))+'\n')
PY
dfx canister call "$CANISTER" get_public_config '()' --network "$IC_HOST" --output json >"$TMP/public-config.json"
dfx canister call "$CANISTER" get_bridge_status '()' --network "$IC_HOST" --output json >"$TMP/status.json"
dfx canister status "$CANISTER" --network "$IC_HOST" --output json >"$TMP/canister-status.json"
if [[ "$MODE" == capture ]]; then
  dfx canister call "$CANISTER" sign_chain_key_challenge "(\"$RELEASE_ID\")" \
    --network "$IC_HOST" --identity "$BRIDGE_DFX_IDENTITY" --output json >"$TMP/chain-key-challenge.json"
fi

python3 - "$PROFILE" "$SNAPSHOT" "$TMP" "$MODE" "${OUTPUT:-}" <<'PY'
import hashlib,json,re,sys,time
from pathlib import Path
p=json.load(open(sys.argv[1])); root=Path(sys.argv[3])
def scalars(v, key):
  out=[]
  if isinstance(v,dict):
    for k,x in v.items():
      if k==key: out.append(x)
      out += scalars(x,key)
  elif isinstance(v,list):
    for x in v: out += scalars(x,key)
  return out
def num(v):
  if isinstance(v,int): return v
  s=str(v).strip().strip('"')
  return int(s,16) if s.startswith('0x') else int(re.sub(r'[^0-9]','',s) or '0')
base_result=json.load(open(root/'base-state.json')); state=base_result['state']; agree=base_result['agreeing_providers']
height=state['height']; bhash=state['hash']; base=state['base_bridge_signer']; runtime_hash=state['bridge_runtime_bytecode_sha256']
delay=state['timelock_minimum_delay_seconds']; self_admin=state['timelock_self_admin']
public=json.load(open(root/'public-config.json')); status=json.load(open(root/'status.json')); cstatus=json.load(open(root/'canister-status.json'))
signers=scalars(public,'expected_bridge_signer')
if not signers: raise SystemExit('Canister public signer missing')
s=signers[0]
if isinstance(s,list): canister='0x'+bytes(s).hex()
else: canister=str(s).lower()
controllers=scalars(cstatus,'controllers')
if not controllers: raise SystemExit('Canister controllers missing')
controllers=controllers[0] if isinstance(controllers[0],list) else controllers
if len(controllers)!=1: raise SystemExit('Canister must have exactly one controller')
controller=str(controllers[0])
module=scalars(cstatus,'module_hash')
module_hash=str(module[0]).lower().removeprefix('0x') if module else ''
sufficient=scalars(status,'sufficient'); sufficient=bool(sufficient[0]) if sufficient else False
paused=scalars(status,'deposits_paused')
if paused != [True]: raise SystemExit('Canister deposits are not paused')
rpc_ids=scalars(public,'evm_rpc_canister_id'); rpc_digests=scalars(public,'rpc_provider_urls_sha256')
if len(rpc_ids)!=1 or str(rpc_ids[0])!=p['evm_rpc_canister_id']: raise SystemExit('Canister EVM RPC ID drift')
expected_rpc_digest=hashlib.sha256(json.dumps([x['url'].strip() for x in p['rpc_providers']],ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
if len(rpc_digests)!=1: raise SystemExit('Canister RPC URL digest missing')
d=rpc_digests[0]; actual_rpc_digest=bytes(d).hex() if isinstance(d,list) else str(d).lower().removeprefix('0x')
if actual_rpc_digest!=expected_rpc_digest: raise SystemExit('Canister RPC URL digest drift')
if sys.argv[4]=='capture':
  challenge=json.load(open(root/'chain-key-challenge.json')); signatures=scalars(challenge,'Ok')
  if len(signatures)!=1 or not isinstance(signatures[0],str) or not re.fullmatch(r'0x[0-9a-f]{130}',signatures[0]): raise SystemExit('Canister chain-key challenge signing failed')
  chain_key_signature=signatures[0]
else: chain_key_signature=json.load(open(sys.argv[2]))['chain_key_eip191_signature']
if runtime_hash.lower()!=p['bridge_runtime_bytecode_sha256'].lower(): raise SystemExit('runtime bytecode drift')
if module_hash.lower()!=p['bridge_canister_wasm_sha256'].lower(): raise SystemExit('Canister Wasm drift')
if controller!=p['root_canister_id']: raise SystemExit('controller drift')
if not sufficient: raise SystemExit('settlement reserve is insufficient')
expected_addresses={
 'base_bridge_signer':p['expected_bridge_signer'],'base_runtime_administrator':p['runtime_administrator'],
 'base_admin_timelock':p['timelock']['address'],'bsns_address':p['bsns_contract'],'bsns_bridge':p['bridge_contract']}
if any(state[k].lower()!=v.lower() for k,v in expected_addresses.items()): raise SystemExit('Base role or bSNS binding drift')
if not state['base_deposit_mints_paused'] or not state['base_withdrawals_paused']: raise SystemExit('Base asset flows are not paused')
if not all(state[k] for k in ('timelock_self_admin','timelock_proposer_authorized','timelock_executor_authorized','timelock_canceller_authorized','timelock_external_admins_absent')): raise SystemExit('Timelock role drift')
if any(state[k] for k in ('timelock_open_proposer','timelock_open_executor','timelock_open_canceller')): raise SystemExit('Timelock has an open role')
if state['bsns_runtime_bytecode_sha256'].lower()!=p['bsns_runtime_bytecode_sha256'].lower() or state['bsns_name']!='KINIC' or state['bsns_symbol']!='KINIC' or state['bsns_decimals']!=p['decimals']: raise SystemExit('bSNS runtime or metadata drift')
out={
 'observed_at_unix':int(time.time()),'chain_id':p['chain_id'],'evm_rpc_canister_id':p['evm_rpc_canister_id'],
 'confirmed_head_block_number':height,'confirmed_head_block_hash':bhash,'canonical':True,'agreeing_providers':agree,'total_providers':3,
 'base_bridge_signer':base,'canister_bridge_signer':canister,'chain_key_eip191_signature':chain_key_signature,
 'bridge_runtime_bytecode_sha256':runtime_hash,'expected_bridge_runtime_bytecode_sha256':p['bridge_runtime_bytecode_sha256'],
 'bridge_canister_wasm_sha256':module_hash,'bridge_canister_id':p['bridge_canister_id'],'timelock_address':p['timelock']['address'],
 'timelock_minimum_delay_seconds':delay,'timelock_self_admin':self_admin,'ic_controller':controller,
 'expected_ic_controller':p['root_canister_id'],'settlement_reserve_sufficient':sufficient,
 'base_deposit_mints_paused':state['base_deposit_mints_paused'],'base_withdrawals_paused':state['base_withdrawals_paused'],
 'canister_deposits_paused':True,'base_runtime_administrator':state['base_runtime_administrator'],
 'timelock_proposer':p['timelock']['proposer'],'timelock_executor':p['timelock']['executor'],'timelock_canceller':p['timelock']['canceller'],
 'timelock_proposer_authorized':state['timelock_proposer_authorized'],'timelock_executor_authorized':state['timelock_executor_authorized'],
 'timelock_canceller_authorized':state['timelock_canceller_authorized'],'timelock_open_proposer':state['timelock_open_proposer'],
 'timelock_open_executor':state['timelock_open_executor'],'timelock_open_canceller':state['timelock_open_canceller'],
 'timelock_external_admins_absent':state['timelock_external_admins_absent'],'bsns_address':state['bsns_address'],
 'bsns_runtime_bytecode_sha256':state['bsns_runtime_bytecode_sha256'],'bsns_name':state['bsns_name'],'bsns_symbol':state['bsns_symbol'],
 'bsns_decimals':state['bsns_decimals'],'bsns_bridge':state['bsns_bridge'],'rpc_provider_urls_sha256':actual_rpc_digest}
mode=sys.argv[4]
if mode=='capture':
  target=Path(sys.argv[5]); tmp=Path(str(target)+'.tmp'); tmp.write_text(json.dumps(out,sort_keys=True,separators=(',',':'))+'\n'); tmp.replace(target)
else:
  old=json.load(open(sys.argv[2]))
  signed=[]
  for i in range(3):
    b=json.load(open(root/f'signed-block-{i}.json')); signed.append((num(b.get('number')),str(b.get('hash','')).lower()))
  expected=(old['confirmed_head_block_number'],old['confirmed_head_block_hash'].lower())
  if signed.count(expected)<2: raise SystemExit('signed Safe block is no longer canonical')
  if out['confirmed_head_block_number'] < old['confirmed_head_block_number']: raise SystemExit('latest Safe head is older than the signed snapshot')
  comparable=lambda x:{k:v for k,v in x.items() if k not in ('observed_at_unix','chain_key_eip191_signature','confirmed_head_block_number','confirmed_head_block_hash','agreeing_providers')}
  if comparable(out)!=comparable(old): raise SystemExit('live state differs from the signed snapshot')
PY
