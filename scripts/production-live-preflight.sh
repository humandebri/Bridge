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
for v in [p['chain_id'],p['evm_rpc_canister_id'],p['bridge_canister_id'],p['bridge_contract'],p['timelock']['address'],p['root_canister_id'],p['bridge_runtime_bytecode_sha256'],p['bridge_canister_wasm_sha256'],p['ic_host'],p['ledger_canister_id'],*map(lambda x:x['url'],p['rpc_providers'])]: print(v)
PY
)
[[ ${#PROFILE_VALUES[@]} -eq 13 ]] || { echo "profile must contain exactly three RPC providers" >&2; exit 1; }
CHAIN_ID=${PROFILE_VALUES[0]}; EVM_RPC_CANISTER=${PROFILE_VALUES[1]}; CANISTER=${PROFILE_VALUES[2]}
BRIDGE=${PROFILE_VALUES[3]}; TIMELOCK=${PROFILE_VALUES[4]}; EXPECTED_CONTROLLER=${PROFILE_VALUES[5]}
EXPECTED_RUNTIME=${PROFILE_VALUES[6]}; EXPECTED_WASM=${PROFILE_VALUES[7]}; IC_HOST=${PROFILE_VALUES[8]}
LEDGER=${PROFILE_VALUES[9]}
RELEASE_ID="$(python3 - "$MANIFEST" <<'PY'
import json,re,sys
value=json.load(open(sys.argv[1]))['release_id']
if not isinstance(value,str) or not re.fullmatch(r'[a-z0-9-]{8,64}',value): raise SystemExit('invalid release_id')
print(value)
PY
)"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-live-preflight.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
python3 - "$PROFILE" "$SNAPSHOT" "$TMP" "$MODE" <<'PY'
import json,subprocess,sys
from pathlib import Path
p=json.load(open(sys.argv[1])); root=Path(sys.argv[3]); mode=sys.argv[4]
signed_height=json.load(open(sys.argv[2]))['finalized_head_block_number'] if mode=='verify' else None
observations=[]
for index,entry in enumerate(p['rpc_providers']):
 try:
  rpc=entry['url']
  chain=subprocess.check_output(['cast','chain-id','--rpc-url',rpc],text=True,stderr=subprocess.DEVNULL).strip()
  if chain!=str(p['chain_id']): raise ValueError('wrong chain')
  finalized=json.loads(subprocess.check_output(['cast','block','finalized','--rpc-url',rpc,'--json'],text=True,stderr=subprocess.DEVNULL))
  observation={'provider_index':index,'chain_id':int(chain),'finalized':finalized}
  if signed_height is not None:
   observation['signed']=json.loads(subprocess.check_output(['cast','block',str(signed_height),'--rpc-url',rpc,'--json'],text=True,stderr=subprocess.DEVNULL))
  observations.append(observation)
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError):
  continue
(root/'provider-observations.json').write_text(json.dumps(observations,sort_keys=True,separators=(',',':'))+'\n')
PY

python3 - "$PROFILE" "$TMP" <<'PY'
import hashlib,json,subprocess,sys
from pathlib import Path
p=json.load(open(sys.argv[1])); root=Path(sys.argv[2]); receipt=json.load(open(Path(sys.argv[1]).with_name('gate-a-receipt.json')))
observations=json.load(open(root/'provider-observations.json'))
blocks=[]
for observation in observations:
 b=observation['finalized']; n=int(str(b['number']),16) if str(b['number']).startswith('0x') else int(b['number']); blocks.append((n,str(b['hash']).lower()))
if not blocks: raise SystemExit('no usable Finalized block observations')
height,block_hash=max(set(blocks),key=blocks.count)
if blocks.count((height,block_hash))<2: raise SystemExit('no Finalized block quorum')
eligible={observation['provider_index'] for observation in observations if (int(str(observation['finalized']['number']),16) if str(observation['finalized']['number']).startswith('0x') else int(observation['finalized']['number']))==height and str(observation['finalized']['hash']).lower()==block_hash}
roles={name:subprocess.check_output(['cast','keccak',name],text=True).strip() for name in ('PROPOSER_ROLE','EXECUTOR_ROLE','CANCELLER_ROLE')}
zero='0x'+'00'*20; zero32='0x'+'00'*32
def run(args): return subprocess.check_output(args,text=True).strip().strip('"')
block_selector=json.dumps({'blockHash':block_hash,'requireCanonical':True},separators=(',',':'))
def call(rpc,address,sig,*args):
 data=run(['cast','calldata',sig,*args])
 request=json.dumps({'to':address,'data':data},separators=(',',':'))
 result=run(['cast','rpc','--rpc-url',rpc,'eth_call',request,block_selector])
 return run(['cast','decode-abi',sig,result])
def code(rpc,address):
 return run(['cast','rpc','--rpc-url',rpc,'eth_getCode',json.dumps(address),block_selector])
def sha_code(value): return hashlib.sha256(bytes.fromhex(value.removeprefix('0x'))).hexdigest()
def number(value):
 return int(str(value),16) if str(value).startswith('0x') else int(value)
def deployment(rpc,tx,address,expected_block):
 receipt=json.loads(run(['cast','receipt',tx,'--rpc-url',rpc,'--json']))
 actual_block=number(receipt.get('blockNumber',-1)); actual_hash=str(receipt.get('blockHash','')).lower()
 status=number(receipt.get('status',0)); contract=str(receipt.get('contractAddress','')).lower()
 canonical=json.loads(run(['cast','block',str(expected_block),'--rpc-url',rpc,'--json']))
 if actual_block!=expected_block or status!=1 or contract!=address.lower() or str(canonical.get('hash','')).lower()!=actual_hash:
  raise ValueError('deployment receipt is not canonical or does not create the reviewed contract')
 return actual_hash
def exact_role_members(rpc,timelock,from_block):
 event_topics={run(['cast','keccak','RoleGranted(bytes32,address,address)']).lower():'grant',run(['cast','keccak','RoleRevoked(bytes32,address,address)']).lower():'revoke'}
 members={zero32:set(),roles['PROPOSER_ROLE'].lower():set(),roles['EXECUTOR_ROLE'].lower():set(),roles['CANCELLER_ROLE'].lower():set()}
 entries=json.loads(run(['cast','logs','--address',timelock,'--from-block',str(from_block),'--to-block',str(height),'--rpc-url',rpc,'--json']))
 for entry in entries:
  event_number=number(entry.get('blockNumber',-1)); event_hash=str(entry.get('blockHash','')).lower()
  canonical=json.loads(run(['cast','block',str(event_number),'--rpc-url',rpc,'--json']))
  if event_number<from_block or event_number>height or not event_hash or str(canonical.get('hash','')).lower()!=event_hash:
   raise ValueError('Timelock role event is not canonical')
  topics=[str(x).lower() for x in entry.get('topics',[])]
  if not topics or topics[0] not in event_topics: continue
  if len(topics)<3: raise ValueError('malformed Timelock role event')
  role=topics[1]; account='0x'+topics[2][-40:]
  if role not in members: raise ValueError('unsupported Timelock role observed')
  if event_topics[topics[0]]=='grant': members[role].add(account)
  else: members[role].discard(account)
 expected={zero32:{timelock.lower()},roles['PROPOSER_ROLE'].lower():{p['timelock']['proposer'].lower()},roles['EXECUTOR_ROLE'].lower():{p['timelock']['executor'].lower()},roles['CANCELLER_ROLE'].lower():{p['timelock']['canceller'].lower()}}
 return members, members==expected
states=[]
for provider_index,rpc_entry in enumerate(p['rpc_providers']):
 if provider_index not in eligible: continue
 try:
  rpc=rpc_entry['url']; bridge=p['bridge_contract']; timelock=p['timelock']['address']
  bsns=call(rpc,bridge,'bsns()(address)').lower()
  external={p['timelock']['proposer'],p['timelock']['executor'],p['timelock']['canceller'],p['base_admin_wallet'],p['runtime_administrator'],p['release_approver'],zero}
  bridge_deployment_hash=deployment(rpc,receipt['bridge_deployment_transaction_hash'],bridge,receipt['bridge_deployment_block_number'])
  timelock_deployment_hash=deployment(rpc,receipt['timelock_deployment_transaction_hash'],timelock,receipt['timelock_deployment_block_number'])
  role_members,roles_exact=exact_role_members(rpc,timelock,receipt['timelock_deployment_block_number'])
  state={
  'height':height,'hash':block_hash,
  'base_bridge_signer':call(rpc,bridge,'bridgeSigner()(address)').lower(),
  'base_runtime_administrator':call(rpc,bridge,'runtimeAdministrator()(address)').lower(),
  'base_admin_timelock':call(rpc,bridge,'baseAdminTimelock()(address)').lower(),
  'bridge_approved_timelock_runtime_code_hash':call(rpc,bridge,'approvedTimelockRuntimeCodeHash()(bytes32)').lower(),
  'base_deposit_mints_paused':call(rpc,bridge,'depositMintsPaused()(bool)').lower()=='true',
  'base_withdrawals_paused':call(rpc,bridge,'withdrawalsPaused()(bool)').lower()=='true',
  'base_service_fee':int(call(rpc,bridge,'serviceFee()(uint256)').split()[0],0),
  'bridge_runtime_bytecode_sha256':sha_code(code(rpc,bridge)),
  'timelock_runtime_code_hash':run(['cast','keccak',code(rpc,timelock)]).lower(),
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
  'timelock_role_members':{k:sorted(v) for k,v in role_members.items()},'timelock_roles_exact':roles_exact,
  'bridge_deployment_transaction_hash':receipt['bridge_deployment_transaction_hash'].lower(),'bridge_deployment_block_number':receipt['bridge_deployment_block_number'],'bridge_deployment_block_hash':bridge_deployment_hash,
  'timelock_deployment_transaction_hash':receipt['timelock_deployment_transaction_hash'].lower(),'timelock_deployment_block_number':receipt['timelock_deployment_block_number'],'timelock_deployment_block_hash':timelock_deployment_hash,
  }
  state['timelock_external_admins_absent']=all(call(rpc,timelock,'hasRole(bytes32,address)(bool)',zero32,a).lower()=='false' for a in external if a.lower()!=timelock.lower())
  final=json.loads(run(['cast','block',str(height),'--rpc-url',rpc,'--json']))
  if str(final.get('hash','')).lower()!=block_hash: raise ValueError('Finalized block changed while Base state was being read')
  states.append(state)
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError):
  continue
if not states: raise SystemExit('no provider completed the bound Finalized state read')
groups={json.dumps(s,sort_keys=True,separators=(',',':')):states.count(s) for s in states}
winner,count=max(groups.items(),key=lambda x:x[1])
if count<2: raise SystemExit('Base state does not have 2-of-3 agreement at the Finalized block')
(root/'base-state.json').write_text(json.dumps({'agreeing_providers':count,'state':json.loads(winner)},sort_keys=True,separators=(',',':'))+'\n')
PY
dfx canister call "$CANISTER" get_public_config '()' --network "$IC_HOST" --output json >"$TMP/public-config.json"
dfx canister call "$CANISTER" get_bridge_status '()' --network "$IC_HOST" --output json >"$TMP/status.json"
dfx canister status "$CANISTER" --network "$IC_HOST" --output json >"$TMP/canister-status.json"
dfx canister call "$LEDGER" icrc1_fee '()' --network "$IC_HOST" --output json >"$TMP/ledger-fee.json"
python3 "$(dirname "$0")/live_fee_guard.py" "$PROFILE" "$TMP/base-state.json" "$TMP/ledger-fee.json" >"$TMP/live-fees.json"
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
live_fees=json.load(open(root/'live-fees.json'))
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
rpc_ids=scalars(public,'evm_rpc_canister_id'); ledger_ids=scalars(public,'ledger_canister_id'); rpc_digests=scalars(public,'rpc_provider_urls_sha256')
if len(rpc_ids)!=1 or str(rpc_ids[0])!=p['evm_rpc_canister_id']: raise SystemExit('Canister EVM RPC ID drift')
if len(ledger_ids)!=1 or str(ledger_ids[0])!=p['ledger_canister_id']: raise SystemExit('Canister Ledger ID drift')
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
if state['timelock_runtime_code_hash'].lower()!=p['timelock']['runtime_code_hash'].lower(): raise SystemExit('Timelock runtime code hash drift')
if state['bridge_approved_timelock_runtime_code_hash'].lower()!=p['timelock']['runtime_code_hash'].lower(): raise SystemExit('Bridge approved Timelock runtime code hash drift')
if module_hash.lower()!=p['bridge_canister_wasm_sha256'].lower(): raise SystemExit('Canister Wasm drift')
if controller!=p['root_canister_id']: raise SystemExit('controller drift')
if not sufficient: raise SystemExit('settlement reserve is insufficient')
expected_addresses={
 'base_bridge_signer':p['expected_bridge_signer'],'base_runtime_administrator':p['runtime_administrator'],
 'base_admin_timelock':p['timelock']['address'],'bsns_address':p['bsns_contract'],'bsns_bridge':p['bridge_contract']}
if any(state[k].lower()!=v.lower() for k,v in expected_addresses.items()): raise SystemExit('Base role or bSNS binding drift')
if not state['base_deposit_mints_paused'] or not state['base_withdrawals_paused']: raise SystemExit('Base asset flows are not paused')
if not all(state[k] for k in ('timelock_self_admin','timelock_proposer_authorized','timelock_executor_authorized','timelock_canceller_authorized','timelock_external_admins_absent','timelock_roles_exact')): raise SystemExit('Timelock role drift')
if any(state[k] for k in ('timelock_open_proposer','timelock_open_executor','timelock_open_canceller')): raise SystemExit('Timelock has an open role')
if state['bsns_runtime_bytecode_sha256'].lower()!=p['bsns_runtime_bytecode_sha256'].lower() or state['bsns_name']!='KINIC' or state['bsns_symbol']!='KINIC' or state['bsns_decimals']!=p['decimals']: raise SystemExit('bSNS runtime or metadata drift')
out={
 'observed_at_unix':int(time.time()),'chain_id':p['chain_id'],'evm_rpc_canister_id':p['evm_rpc_canister_id'],
 'finalized_head_block_number':height,'finalized_head_block_hash':bhash,'canonical':True,'agreeing_providers':agree,'total_providers':3,
 'base_bridge_signer':base,'canister_bridge_signer':canister,'chain_key_eip191_signature':chain_key_signature,
 'bridge_runtime_bytecode_sha256':runtime_hash,'expected_bridge_runtime_bytecode_sha256':p['bridge_runtime_bytecode_sha256'],
 'bridge_canister_wasm_sha256':module_hash,'bridge_canister_id':p['bridge_canister_id'],'timelock_address':p['timelock']['address'],
 'timelock_runtime_code_hash':state['timelock_runtime_code_hash'],
 'bridge_approved_timelock_runtime_code_hash':state['bridge_approved_timelock_runtime_code_hash'],
 'timelock_minimum_delay_seconds':delay,'timelock_self_admin':self_admin,'ic_controller':controller,
 'expected_ic_controller':p['root_canister_id'],'settlement_reserve_sufficient':sufficient,
 'ledger_fee':live_fees['ledger_fee'],'base_service_fee':live_fees['base_service_fee'],
 'base_deposit_mints_paused':state['base_deposit_mints_paused'],'base_withdrawals_paused':state['base_withdrawals_paused'],
 'canister_deposits_paused':True,'base_runtime_administrator':state['base_runtime_administrator'],
 'timelock_proposer':p['timelock']['proposer'],'timelock_executor':p['timelock']['executor'],'timelock_canceller':p['timelock']['canceller'],
 'timelock_proposer_authorized':state['timelock_proposer_authorized'],'timelock_executor_authorized':state['timelock_executor_authorized'],
 'timelock_canceller_authorized':state['timelock_canceller_authorized'],'timelock_open_proposer':state['timelock_open_proposer'],
 'timelock_open_executor':state['timelock_open_executor'],'timelock_open_canceller':state['timelock_open_canceller'],
 'timelock_external_admins_absent':state['timelock_external_admins_absent'],'bsns_address':state['bsns_address'],
 'timelock_roles_exact':state['timelock_roles_exact'],'bridge_deployment_transaction_hash':state['bridge_deployment_transaction_hash'],
 'bridge_deployment_block_number':state['bridge_deployment_block_number'],'bridge_deployment_block_hash':state['bridge_deployment_block_hash'],
 'timelock_deployment_transaction_hash':state['timelock_deployment_transaction_hash'],'timelock_deployment_block_number':state['timelock_deployment_block_number'],
 'timelock_deployment_block_hash':state['timelock_deployment_block_hash'],
 'bsns_runtime_bytecode_sha256':state['bsns_runtime_bytecode_sha256'],'bsns_name':state['bsns_name'],'bsns_symbol':state['bsns_symbol'],
 'bsns_decimals':state['bsns_decimals'],'bsns_bridge':state['bsns_bridge'],'rpc_provider_urls_sha256':actual_rpc_digest}
mode=sys.argv[4]
if mode=='capture':
  target=Path(sys.argv[5]); tmp=Path(str(target)+'.tmp'); tmp.write_text(json.dumps(out,sort_keys=True,separators=(',',':'))+'\n'); tmp.replace(target)
else:
  old=json.load(open(sys.argv[2]))
  signed=[]
  for observation in json.load(open(root/'provider-observations.json')):
    if 'signed' in observation:
      b=observation['signed']; signed.append((num(b.get('number')),str(b.get('hash','')).lower()))
  expected=(old['finalized_head_block_number'],old['finalized_head_block_hash'].lower())
  if signed.count(expected)<2: raise SystemExit('signed Finalized block is no longer canonical')
  if out['finalized_head_block_number'] < old['finalized_head_block_number']: raise SystemExit('latest Finalized head is older than the signed snapshot')
  comparable=lambda x:{k:v for k,v in x.items() if k not in ('observed_at_unix','chain_key_eip191_signature','finalized_head_block_number','finalized_head_block_hash','agreeing_providers')}
  if comparable(out)!=comparable(old): raise SystemExit('live state differs from the signed snapshot')
PY
