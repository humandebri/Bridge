#!/usr/bin/env bash
# Capture live state and verify the Mint Signer against the reviewed profile.
set -euo pipefail

MODE="${1:-verify}"
if [[ "$MODE" == verify-gate-a ]]; then
  BUNDLE="${2:?usage: production-live-preflight.sh verify-gate-a BUNDLE}"
  DRILL="$BUNDLE/monitor-drill.json"
  [[ -f "$DRILL" ]] || { echo "monitor drill evidence is missing" >&2; exit 1; }
  for tool in cast python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
  : "${BRIDGE_GATE_A_RPC_URL_1:?missing BRIDGE_GATE_A_RPC_URL_1}"
  : "${BRIDGE_GATE_A_RPC_URL_2:?missing BRIDGE_GATE_A_RPC_URL_2}"
  : "${BRIDGE_GATE_A_RPC_URL_3:?missing BRIDGE_GATE_A_RPC_URL_3}"
  python3 - "$DRILL" "$BRIDGE_GATE_A_RPC_URL_1" "$BRIDGE_GATE_A_RPC_URL_2" "$BRIDGE_GATE_A_RPC_URL_3" <<'PY'
import hashlib,json,re,subprocess,sys
from urllib.parse import urlsplit

drill=json.load(open(sys.argv[1],encoding='utf-8')); providers=sys.argv[2:]
if len(set(providers))!=3: raise SystemExit('Gate A requires exactly three distinct Base RPC providers')
for rpc in providers:
 p=urlsplit(rpc)
 if p.scheme!='https' or not p.hostname or p.username or p.password or p.query or p.fragment:
  raise SystemExit('Gate A RPC providers must be credential-free HTTPS URLs')
digest=hashlib.sha256(json.dumps(providers,separators=(',',':')).encode()).hexdigest()
if digest.lower()!=str(drill['rpc_provider_urls_sha256']).lower():
 raise SystemExit('Gate A RPC provider digest differs from the rehearsal binding')

def run(args): return subprocess.check_output(args,text=True,stderr=subprocess.DEVNULL).strip()
def number(value):
 if isinstance(value,int): return value
 value=str(value); return int(value,16) if value.startswith('0x') else int(value)
def finalized_block(rpc): return json.loads(run(['cast','block','finalized','--rpc-url',rpc,'--json']))
def verify_all_provider_chain_ids(providers,expected,context):
 for index,rpc in enumerate(providers):
  try: chain_id=int(run(['cast','chain-id','--rpc-url',rpc]))
  except (OSError,subprocess.CalledProcessError,ValueError):
   raise SystemExit(f'{context} RPC provider {index} chain ID check failed')
  if chain_id!=expected:
   raise SystemExit(f'{context} RPC provider {index} returned chain ID {chain_id}; expected {expected}')
def canonical_probe(rpc,address,signature,block_hash,expected_block=None):
 if not re.fullmatch(r'0x[0-9a-f]{64}',block_hash): raise ValueError('invalid canonical probe block hash')
 data=run(['cast','calldata',signature])
 request=json.dumps({'to':address,'data':data},separators=(',',':'))
 selector=json.dumps({'blockHash':block_hash,'requireCanonical':True},separators=(',',':'))
 raw=run(['cast','rpc','--rpc-url',rpc,'eth_call',request,selector])
 try: value=json.loads(raw)
 except json.JSONDecodeError: value=raw
 if not isinstance(value,str) or not value.startswith('0x') or not re.fullmatch(r'[0-9a-fA-F]*',value[2:]):
  raise ValueError('malformed canonical probe response')
 payload=value[2:]
 if signature=='bridgeSnapshot()':
  if len(payload)!=12*64: raise ValueError('malformed bridgeSnapshot response')
  if expected_block is not None and int(payload[:64],16)!=expected_block:
   raise ValueError('bridgeSnapshot block number mismatch')
 elif signature=='getMinDelay()':
  if len(payload)!=64: raise ValueError('malformed getMinDelay response')
 else:
  raise ValueError('unsupported canonical probe')
heads=[]
verify_all_provider_chain_ids(providers,drill['base_chain_id'],'Gate A')
for index,rpc in enumerate(providers):
 try:
  value=finalized_block(rpc); heads.append((index,number(value['number']),str(value['hash']).lower()))
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError): pass
if not heads: raise SystemExit('Gate A has no usable Finalized provider')
pairs=[(height,block_hash) for _,height,block_hash in heads]
winner=max(set(pairs),key=pairs.count)
eligible={index for index,height,block_hash in heads if (height,block_hash)==winner}
if len(eligible)<2: raise SystemExit('Gate A Finalized head has no 2-of-3 agreement')

event_topics={
 'PauseDepositMints':run(['cast','keccak','DepositMintsPaused(address)']).lower(),
 'PauseWithdrawals':run(['cast','keccak','WithdrawalsPaused(address)']).lower(),
 'CancelTimelock':run(['cast','keccak','Cancelled(bytes32)']).lower(),
}
for action in drill['base_actions']:
 matches=0
 for index in eligible:
  rpc=providers[index]
  try:
   receipt=json.loads(run(['cast','receipt',action['transaction_hash'],'--rpc-url',rpc,'--json']))
   tx=json.loads(run(['cast','tx',action['transaction_hash'],'--rpc-url',rpc,'--json']))
   receipt_hash=str(receipt.get('blockHash','')).lower()
   receipt_number=number(receipt.get('blockNumber',-1)); status=number(receipt.get('status',0))
   target=str(tx.get('to','')).lower(); calldata=str(tx.get('input',tx.get('data',''))).lower()
   event=event_topics[action['kind']]; expected_address=action['target'].lower()
   log_match=any(str(log.get('address','')).lower()==expected_address and
     [str(topic).lower() for topic in log.get('topics',[])][:1]==[event]
     for log in receipt.get('logs',[]))
   if (receipt_number==action['block_number'] and status==1 and
       receipt_hash==action['block_hash'].lower() and
       target==expected_address and calldata==action['calldata_hex'].lower() and
       winner[0]>=receipt_number and log_match):
    signature='getMinDelay()' if action['kind']=='CancelTimelock' else 'bridgeSnapshot()'
    canonical_probe(rpc,expected_address,signature,receipt_hash,receipt_number)
    matches+=1
  except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError): pass
 if matches<2: raise SystemExit(f"Gate A action {action['kind']} lacks 2-of-3 canonical Finalized agreement")
print('gate_a_base=pass')
PY
  exit 0
elif [[ "$MODE" == verify-activation ]]; then
  PHASE="${2:?usage: production-live-preflight.sh verify-activation PHASE BUNDLE OPERATION_ID}"
  BUNDLE="${3:?usage: production-live-preflight.sh verify-activation PHASE BUNDLE OPERATION_ID}"
  OPERATION_ID="${4:?usage: production-live-preflight.sh verify-activation PHASE BUNDLE OPERATION_ID}"
  [[ "$PHASE" == schedule || "$PHASE" == execute ]] || { echo "invalid activation phase" >&2; exit 1; }
  [[ -f "$BUNDLE/profile.json" ]] || { echo "activation profile is missing" >&2; exit 1; }
  [[ "$OPERATION_ID" =~ ^0x[0-9a-fA-F]{64}$ ]] || { echo "invalid activation operation ID" >&2; exit 1; }
  for tool in cast python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
  python3 - "$PHASE" "$BUNDLE/profile.json" "$OPERATION_ID" <<'PY'
import json,subprocess,sys

phase,profile_path,operation_id=sys.argv[1:]
profile=json.load(open(profile_path,encoding='utf-8'))
providers=[item['url'] for item in profile['rpc_providers']]
bridge=profile['bridge_contract']; timelock=profile['timelock']['address']
if len(providers)!=3 or len(set(providers))!=3: raise SystemExit('activation verification requires three distinct RPC providers')
def run(args): return subprocess.check_output(args,text=True,stderr=subprocess.DEVNULL).strip()
def number(value):
 if isinstance(value,int): return value
 value=str(value); return int(value,16) if value.startswith('0x') else int(value)
def finalized_block(rpc): return json.loads(run(['cast','block','finalized','--rpc-url',rpc,'--json']))
def verify_all_provider_chain_ids(providers,expected,context):
 for index,rpc in enumerate(providers):
  try: chain_id=int(run(['cast','chain-id','--rpc-url',rpc]))
  except (OSError,subprocess.CalledProcessError,ValueError):
   raise SystemExit(f'{context} RPC provider {index} chain ID check failed')
  if chain_id!=expected:
   raise SystemExit(f'{context} RPC provider {index} returned chain ID {chain_id}; expected {expected}')
def bool_call(rpc,address,signature,*args):
 data=run(['cast','calldata',signature,*args])
 request=json.dumps({'to':address,'data':data},separators=(',',':'))
 selector=json.dumps({'blockHash':winner[1],'requireCanonical':True},separators=(',',':'))
 raw=run(['cast','rpc','--rpc-url',rpc,'eth_call',request,selector])
 try: value=json.loads(raw)
 except json.JSONDecodeError: value=raw
 if not isinstance(value,str) or len(value)!=66 or not value.startswith('0x'):
  raise ValueError('malformed ABI bool response')
 decoded=int(value,16)
 if decoded not in (0,1): raise ValueError('non-boolean ABI response')
 return decoded==1
heads=[]
verify_all_provider_chain_ids(providers,profile['chain_id'],'activation')
for index,rpc in enumerate(providers):
 try:
  value=finalized_block(rpc); heads.append((index,number(value['number']),str(value['hash']).lower()))
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError): pass
if not heads: raise SystemExit('activation verification has no usable Finalized provider')
pairs=[(height,block_hash) for _,height,block_hash in heads]
winner=max(set(pairs),key=pairs.count)
eligible=[index for index,height,block_hash in heads if (height,block_hash)==winner]
if len(eligible)<2: raise SystemExit('activation Finalized head has no 2-of-3 agreement')

matches=0
for index in eligible:
 rpc=providers[index]
 try:
  deposits=bool_call(rpc,bridge,'depositMintsPaused()(bool)')
  withdrawals=bool_call(rpc,bridge,'withdrawalsPaused()(bool)')
  signature='isOperationPending(bytes32)(bool)' if phase=='schedule' else 'isOperationDone(bytes32)(bool)'
  operation=bool_call(rpc,timelock,signature,operation_id)
  expected_paused=phase=='schedule'
  if deposits==expected_paused and withdrawals==expected_paused and operation: matches+=1
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError): pass
if matches<2: raise SystemExit('activation Base postcondition lacks 2-of-3 canonical Finalized agreement')
print(f'activation_base=pass phase={phase} operation_id={operation_id.lower()} finalized_block={winner[0]} finalized_hash={winner[1]}')
PY
  exit 0
elif [[ "$MODE" == capture ]]; then
  BUNDLE="${2:?usage: production-live-preflight.sh capture BUNDLE OUTPUT}"
  OUTPUT="${3:?usage: production-live-preflight.sh capture BUNDLE OUTPUT}"
elif [[ "$MODE" == verify ]]; then
  BUNDLE="${2:?usage: production-live-preflight.sh verify BUNDLE}"
  OUTPUT=""
else
  echo "usage: production-live-preflight.sh {capture BUNDLE OUTPUT|verify BUNDLE|verify-gate-a BUNDLE|verify-activation PHASE BUNDLE OPERATION_ID}" >&2; exit 2
fi
PROFILE="$BUNDLE/profile.json"
SNAPSHOT="$BUNDLE/signer-snapshot.json"
MANIFEST="$BUNDLE/release-manifest.json"
[[ -f "$PROFILE" && -f "$MANIFEST" && -f "$BUNDLE/controller-handover.json" ]] || {
  echo "profile, manifest, or controller handover evidence is missing" >&2; exit 1;
}
if [[ "$MODE" == verify && ! -f "$SNAPSHOT" ]]; then
  echo "signer snapshot is missing" >&2; exit 1
fi
: "${BRIDGE_ICP_IDENTITY:?BRIDGE_ICP_IDENTITY must name the reviewed identity used to resolve the production environment}"
for tool in cast icp python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

PROFILE_VALUES=()
while IFS= read -r line; do PROFILE_VALUES[${#PROFILE_VALUES[@]}]="$line"; done < <(python3 - "$PROFILE" <<'PY'
import json,sys
p=json.load(open(sys.argv[1]))
for v in [p['chain_id'],p['evm_rpc_canister_id'],p['bridge_canister_id'],p['bridge_contract'],p['timelock']['address'],p['root_canister_id'],p['bridge_runtime_bytecode_sha256'],p['bridge_canister_wasm_sha256'],p['ledger_canister_id'],*map(lambda x:x['url'],p['rpc_providers'])]: print(v)
PY
)
[[ ${#PROFILE_VALUES[@]} -eq 12 ]] || { echo "profile must contain exactly three RPC providers" >&2; exit 1; }
CHAIN_ID=${PROFILE_VALUES[0]}; EVM_RPC_CANISTER=${PROFILE_VALUES[1]}; CANISTER=${PROFILE_VALUES[2]}
BRIDGE=${PROFILE_VALUES[3]}; TIMELOCK=${PROFILE_VALUES[4]}; EXPECTED_CONTROLLER=${PROFILE_VALUES[5]}
EXPECTED_RUNTIME=${PROFILE_VALUES[6]}; EXPECTED_WASM=${PROFILE_VALUES[7]}; LEDGER=${PROFILE_VALUES[8]}
[[ "$(icp canister status bridge-canister -e production -i --identity "$BRIDGE_ICP_IDENTITY")" == "$CANISTER" ]] || {
  echo "production ICP environment does not map the reviewed Bridge Canister" >&2; exit 1;
}
TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-live-preflight.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
python3 - "$PROFILE" "$SNAPSHOT" "$TMP" "$MODE" <<'PY'
import json,re,subprocess,sys
from pathlib import Path
p=json.load(open(sys.argv[1])); root=Path(sys.argv[3]); mode=sys.argv[4]
snapshot=json.load(open(sys.argv[2])) if mode=='verify' else None
snapshot_height=snapshot['finalized_head_block_number'] if snapshot else None
snapshot_hash=str(snapshot['finalized_head_block_hash']).lower() if snapshot else None
def verify_all_provider_chain_ids(providers,expected,context):
 for index,rpc in enumerate(providers):
  try:
   chain=subprocess.check_output(['cast','chain-id','--rpc-url',rpc],text=True,stderr=subprocess.DEVNULL).strip()
  except (OSError,subprocess.CalledProcessError):
   raise SystemExit(f'{context} RPC provider {index} chain ID check failed')
  if chain!=str(expected):
   raise SystemExit(f'{context} RPC provider {index} returned chain ID {chain}; expected {expected}')
def bridge_snapshot_probe(rpc,block_hash,expected_block):
 if not re.fullmatch(r'0x[0-9a-f]{64}',block_hash): raise ValueError('invalid snapshot block hash')
 data=subprocess.check_output(['cast','calldata','bridgeSnapshot()'],text=True,stderr=subprocess.DEVNULL).strip()
 request=json.dumps({'to':p['bridge_contract'],'data':data},separators=(',',':'))
 selector=json.dumps({'blockHash':block_hash,'requireCanonical':True},separators=(',',':'))
 raw=subprocess.check_output(['cast','rpc','--rpc-url',rpc,'eth_call',request,selector],text=True,stderr=subprocess.DEVNULL).strip()
 try: value=json.loads(raw)
 except json.JSONDecodeError: value=raw
 if not isinstance(value,str) or not value.startswith('0x') or not re.fullmatch(r'[0-9a-fA-F]{768}',value[2:]):
  raise ValueError('malformed bridgeSnapshot response')
 if int(value[2:66],16)!=expected_block: raise ValueError('bridgeSnapshot block number mismatch')
observations=[]
verify_all_provider_chain_ids([entry['url'] for entry in p['rpc_providers']],p['chain_id'],'live preflight')
for index,entry in enumerate(p['rpc_providers']):
 try:
  rpc=entry['url']
  finalized=json.loads(subprocess.check_output(['cast','block','finalized','--rpc-url',rpc,'--json'],text=True,stderr=subprocess.DEVNULL))
  observation={'provider_index':index,'chain_id':p['chain_id'],'finalized':finalized}
  if snapshot_height is not None:
   bridge_snapshot_probe(rpc,snapshot_hash,snapshot_height)
   observation['snapshot_probe']={'number':snapshot_height,'hash':snapshot_hash}
  observations.append(observation)
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError):
  continue
(root/'provider-observations.json').write_text(json.dumps(observations,sort_keys=True,separators=(',',':'))+'\n')
PY

python3 - "$PROFILE" "$TMP" <<'PY'
import hashlib,json,re,subprocess,sys
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
def raw_call_at_hash(rpc,address,sig,at_hash,*args):
 data=run(['cast','calldata',sig,*args])
 request=json.dumps({'to':address,'data':data},separators=(',',':'))
 selector=json.dumps({'blockHash':at_hash,'requireCanonical':True},separators=(',',':'))
 return run(['cast','rpc','--rpc-url',rpc,'eth_call',request,selector])
def call(rpc,address,sig,*args):
 result=raw_call_at_hash(rpc,address,sig,block_hash,*args)
 return run(['cast','decode-abi',sig,result])
def canonical_word_probe(rpc,address,sig,at_hash):
 if not re.fullmatch(r'0x[0-9a-f]{64}',at_hash): raise ValueError('invalid canonical probe block hash')
 result=raw_call_at_hash(rpc,address,sig,at_hash)
 if not re.fullmatch(r'0x[0-9a-fA-F]{64}',result): raise ValueError('malformed canonical word probe')
 return int(result,16)
def canonical_bridge_probe(rpc,address,at_hash,expected_block=None):
 if not re.fullmatch(r'0x[0-9a-f]{64}',at_hash): raise ValueError('invalid canonical probe block hash')
 result=raw_call_at_hash(rpc,address,'bridgeSnapshot()',at_hash)
 if not re.fullmatch(r'0x[0-9a-fA-F]{768}',result): raise ValueError('malformed bridgeSnapshot probe')
 observed_block=int(result[2:66],16)
 if expected_block is not None and observed_block!=expected_block:
  raise ValueError('bridgeSnapshot block number mismatch')
 return observed_block
def code(rpc,address):
 selector=json.dumps({'blockHash':block_hash,'requireCanonical':True},separators=(',',':'))
 return run(['cast','rpc','--rpc-url',rpc,'eth_getCode',json.dumps(address),selector])
def sha_code(value): return hashlib.sha256(bytes.fromhex(value.removeprefix('0x'))).hexdigest()
def number(value):
 return int(str(value),16) if str(value).startswith('0x') else int(value)
def deployment(rpc,tx,address,expected_block,probe):
 receipt=json.loads(run(['cast','receipt',tx,'--rpc-url',rpc,'--json']))
 actual_block=number(receipt.get('blockNumber',-1)); actual_hash=str(receipt.get('blockHash','')).lower()
 status=number(receipt.get('status',0)); contract=str(receipt.get('contractAddress','')).lower()
 if actual_block!=expected_block or status!=1 or contract!=address.lower():
  raise ValueError('deployment receipt is not canonical or does not create the reviewed contract')
 if probe=='bridge': canonical_bridge_probe(rpc,address,actual_hash,actual_block)
 elif probe=='timelock': canonical_word_probe(rpc,address,'getMinDelay()',actual_hash)
 else: raise ValueError('unsupported deployment probe')
 return actual_hash
def exact_role_members(rpc,timelock,from_block):
 event_topics={run(['cast','keccak','RoleGranted(bytes32,address,address)']).lower():'grant',run(['cast','keccak','RoleRevoked(bytes32,address,address)']).lower():'revoke'}
 members={zero32:set(),roles['PROPOSER_ROLE'].lower():set(),roles['EXECUTOR_ROLE'].lower():set(),roles['CANCELLER_ROLE'].lower():set()}
 entries=[]
 canonical_event_blocks=set()
 # Provider eth_getLogs limits vary. Bounded pages also keep a single response
 # from silently truncating the complete role history.
 page_start=from_block
 while page_start<=height:
  page_end=min(page_start+49_999,height)
  page=json.loads(run(['cast','logs','--address',timelock,'--from-block',str(page_start),'--to-block',str(page_end),'--rpc-url',rpc,'--json']))
  if not isinstance(page,list): raise ValueError('Timelock role log response is not a list')
  entries.extend(page); page_start=page_end+1
 entries.sort(key=lambda e:(number(e.get('blockNumber',-1)),number(e.get('transactionIndex',0)),number(e.get('logIndex',0))))
 for entry in entries:
  event_number=number(entry.get('blockNumber',-1)); event_hash=str(entry.get('blockHash','')).lower()
  if event_number<from_block or event_number>height or not event_hash:
   raise ValueError('Timelock role event is not canonical')
  event_block=(event_number,event_hash)
  if event_block not in canonical_event_blocks:
   canonical_word_probe(rpc,timelock,'getMinDelay()',event_hash)
   canonical_event_blocks.add(event_block)
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
  external={p['timelock']['proposer'],p['timelock']['executor'],p['timelock']['canceller'],p['governance_operator'],zero}
  bridge_deployment_hash=deployment(rpc,receipt['bridge_deployment_transaction_hash'],bridge,receipt['bridge_deployment_block_number'],'bridge')
  timelock_deployment_hash=deployment(rpc,receipt['timelock_deployment_transaction_hash'],timelock,receipt['timelock_deployment_block_number'],'timelock')
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
  canonical_bridge_probe(rpc,bridge,block_hash,height)
  states.append(state)
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError):
  continue
if not states: raise SystemExit('no provider completed the bound Finalized state read')
groups={json.dumps(s,sort_keys=True,separators=(',',':')):states.count(s) for s in states}
winner,count=max(groups.items(),key=lambda x:x[1])
if count<2: raise SystemExit('Base state does not have 2-of-3 agreement at the Finalized block')
(root/'base-state.json').write_text(json.dumps({'agreeing_providers':count,'state':json.loads(winner)},sort_keys=True,separators=(',',':'))+'\n')
PY
icp canister call bridge-canister get_public_config '()' -e production --query --json >"$TMP/public-config.json"
icp canister call bridge-canister get_bridge_status '()' -e production --json >"$TMP/status.json"
icp canister status bridge-canister -e production --public --json >"$TMP/canister-status.json"
icp canister call "$LEDGER" icrc1_fee '()' -n ic --json >"$TMP/ledger-fee.json"
cp "$BUNDLE/controller-handover.json" "$TMP/controller-handover.json"
python3 "$(dirname "$0")/live_fee_guard.py" "$PROFILE" "$TMP/base-state.json" "$TMP/ledger-fee.json" >"$TMP/live-fees.json"
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
def one(key):
  values=scalars(public,key)
  if len(values)!=1: raise SystemExit(f'Canister public config has no unique {key}')
  return values[0]
def address_value(value):
  return '0x'+bytes(value).hex() if isinstance(value,list) else str(value).lower()
base_result=json.load(open(root/'base-state.json')); state=base_result['state']; agree=base_result['agreeing_providers']
height=state['height']; bhash=state['hash']; base=state['base_bridge_signer']; runtime_hash=state['bridge_runtime_bytecode_sha256']
delay=state['timelock_minimum_delay_seconds']; self_admin=state['timelock_self_admin']
public=json.load(open(root/'public-config.json')); status=json.load(open(root/'status.json')); cstatus=json.load(open(root/'canister-status.json'))
handover=json.load(open(root/'controller-handover.json'))
live_fees=json.load(open(root/'live-fees.json'))
signers=scalars(public,'expected_bridge_signer')
if not signers: raise SystemExit('Canister public signer missing')
s=signers[0]
if isinstance(s,list): canister='0x'+bytes(s).hex()
else: canister=str(s).lower()
expected_signer=p['expected_bridge_signer'].lower()
if base.lower()!=expected_signer or canister.lower()!=expected_signer or base.lower()!=canister.lower():
  raise SystemExit('profile, Canister public config, and Finalized Base signer mismatch')
evidence_controllers=handover.get('final_controllers')
if evidence_controllers != [p['root_canister_id']]: raise SystemExit('handover evidence does not bind the SNS Root-only controller set')
controller_values=scalars(cstatus,'controllers')
if len(controller_values)!=1 or not isinstance(controller_values[0],list): raise SystemExit('live Canister controller set is unavailable')
controllers=[str(value) for value in controller_values[0]]
if controllers != [p['root_canister_id']]: raise SystemExit('live Canister controller set is not SNS Root-only')
controller=controllers[0]
module=scalars(cstatus,'module_hash')
module_hash=str(module[0]).lower().removeprefix('0x') if module else ''
sufficient=scalars(status,'sufficient'); sufficient=bool(sufficient[0]) if sufficient else False
paused=scalars(status,'deposits_paused')
if paused != [True]: raise SystemExit('Canister deposits are not paused')
rpc_ids=scalars(public,'evm_rpc_canister_id'); ledger_ids=scalars(public,'ledger_canister_id'); rpc_digests=scalars(public,'rpc_provider_urls_sha256')
deployment_instances=scalars(public,'deployment_instance_id')
minimum_withdrawal_ids=scalars(public,'minimum_withdrawal_id')
if len(rpc_ids)!=1 or str(rpc_ids[0])!=p['evm_rpc_canister_id']: raise SystemExit('Canister EVM RPC ID drift')
if len(ledger_ids)!=1 or str(ledger_ids[0])!=p['ledger_canister_id']: raise SystemExit('Canister Ledger ID drift')
expected_rpc_digest=hashlib.sha256(b'[]').hexdigest()
if len(rpc_digests)!=1: raise SystemExit('Canister RPC URL digest missing')
d=rpc_digests[0]; actual_rpc_digest=bytes(d).hex() if isinstance(d,list) else str(d).lower().removeprefix('0x')
if actual_rpc_digest!=expected_rpc_digest: raise SystemExit('Canister RPC URL digest drift')
if len(deployment_instances)!=1: raise SystemExit('Canister deployment instance ID missing')
deployment_instance=deployment_instances[0]
if isinstance(deployment_instance,list): deployment_instance='0x'+bytes(deployment_instance).hex()
else: deployment_instance=str(deployment_instance).lower()
if not re.fullmatch(r'0x[0-9a-f]{64}',deployment_instance) or int(deployment_instance[2:],16)==0: raise SystemExit('Canister deployment instance ID invalid')
if len(minimum_withdrawal_ids)!=1: raise SystemExit('Canister minimum withdrawal ID missing')
minimum_withdrawal_id=minimum_withdrawal_ids[0]
if isinstance(minimum_withdrawal_id,list): minimum_withdrawal_id='0x'+bytes(minimum_withdrawal_id).hex()
else: minimum_withdrawal_id=str(minimum_withdrawal_id).lower()
if not re.fullmatch(r'0x[0-9a-f]{64}',minimum_withdrawal_id) or int(minimum_withdrawal_id[2:],16)==0: raise SystemExit('Canister minimum withdrawal ID invalid')
governance_replacement=one('governance_replacement'); governance_evm_fee=one('governance_evm_fee'); fee_recipient=one('fee_recipient')
if not isinstance(governance_replacement,dict) or not isinstance(governance_evm_fee,dict) or not isinstance(fee_recipient,dict): raise SystemExit('Canister public config nested values are malformed')
subaccount=fee_recipient.get('subaccount',[])
public_config={
 'base_chain_id':num(one('base_chain_id')),'bridge_contract':address_value(one('bridge_contract')),
 'timelock_contract':address_value(one('timelock_contract')),'ledger_canister_id':str(one('ledger_canister_id')),
 'deployment_instance_id':deployment_instance,
 'minimum_withdrawal_id':minimum_withdrawal_id,
 'index_canister_id':str(one('index_canister_id')),'schema_version':num(one('schema_version')),
 'expected_bridge_signer':address_value(one('expected_bridge_signer')),'governance_operator':address_value(one('governance_operator')),
 'evm_rpc_canister_id':str(one('evm_rpc_canister_id')),'rpc_provider_urls_sha256':actual_rpc_digest,
 'deposit_rate_limit_window_seconds':num(one('deposit_rate_limit_window_seconds')),
 'deposit_rate_limit_global':num(one('deposit_rate_limit_global')),'deposit_rate_limit_per_principal':num(one('deposit_rate_limit_per_principal')),
 'notification_rate_limit_window_seconds':num(one('notification_rate_limit_window_seconds')),
 'notification_rate_limit_global':num(one('notification_rate_limit_global')),
 'notification_ingestion_rate_limit_global':num(one('notification_ingestion_rate_limit_global')),
 'settlement_rate_limit_window_seconds':num(one('settlement_rate_limit_window_seconds')),
 'settlement_rate_limit_global':num(one('settlement_rate_limit_global')),'settlement_rate_limit_per_principal':num(one('settlement_rate_limit_per_principal')),
 'settlement_rate_limit_per_record':num(one('settlement_rate_limit_per_record')),
 'settlement_retry_interval_seconds':num(one('settlement_retry_interval_seconds')),
 'governance_evm_fee':{k:(str(num(v)) if k in ('gas_limit_ceiling','max_fee_per_gas_ceiling','max_priority_fee_per_gas_ceiling','l1_fee_per_transaction_ceiling_wei') else num(v)) for k,v in governance_evm_fee.items()},
 'governance_replacement':{k:num(v) for k,v in governance_replacement.items()},
 'cycles_floor':str(num(one('cycles_floor'))),
 'settlement_cycle_ceiling':str(num(one('settlement_cycle_ceiling'))),'governance_principal':str(one('governance_principal')),
 'pause_principal':str(one('pause_principal')),'fee_recipient':{'owner':str(fee_recipient.get('owner','')),
 'subaccount_hex':bytes(subaccount).hex() if isinstance(subaccount,list) else str(subaccount).lower().removeprefix('0x')}}
if runtime_hash.lower()!=p['bridge_runtime_bytecode_sha256'].lower(): raise SystemExit('runtime bytecode drift')
if state['timelock_runtime_code_hash'].lower()!=p['timelock']['runtime_code_hash'].lower(): raise SystemExit('Timelock runtime code hash drift')
if state['bridge_approved_timelock_runtime_code_hash'].lower()!=p['timelock']['runtime_code_hash'].lower(): raise SystemExit('Bridge approved Timelock runtime code hash drift')
if delay!=p['timelock']['minimum_delay_seconds']: raise SystemExit('Timelock minimum delay drift')
if module_hash.lower()!=p['bridge_canister_wasm_sha256'].lower(): raise SystemExit('Canister Wasm drift')
if controller!=p['root_canister_id']: raise SystemExit('controller drift')
if not sufficient: raise SystemExit('settlement reserve is insufficient')
expected_addresses={
 'base_bridge_signer':p['expected_bridge_signer'],'base_runtime_administrator':p['governance_operator'],
 'base_admin_timelock':p['timelock']['address'],'bsns_address':p['bsns_contract'],'bsns_bridge':p['bridge_contract']}
if any(state[k].lower()!=v.lower() for k,v in expected_addresses.items()): raise SystemExit('Base role or bSNS binding drift')
if not state['base_deposit_mints_paused'] or not state['base_withdrawals_paused']: raise SystemExit('Base asset flows are not paused')
if not all(state[k] for k in ('timelock_self_admin','timelock_proposer_authorized','timelock_executor_authorized','timelock_canceller_authorized','timelock_external_admins_absent','timelock_roles_exact')): raise SystemExit('Timelock role drift')
if any(state[k] for k in ('timelock_open_proposer','timelock_open_executor','timelock_open_canceller')): raise SystemExit('Timelock has an open role')
if state['bsns_runtime_bytecode_sha256'].lower()!=p['bsns_runtime_bytecode_sha256'].lower() or state['bsns_name']!='KINIC' or state['bsns_symbol']!='KINIC' or state['bsns_decimals']!=p['decimals']: raise SystemExit('bSNS runtime or metadata drift')
out={
 'schema_version':2,'observed_at_unix':int(time.time()),'chain_id':p['chain_id'],'evm_rpc_canister_id':p['evm_rpc_canister_id'],
 'finalized_head_block_number':height,'finalized_head_block_hash':bhash,'canonical':True,'agreeing_providers':agree,'total_providers':3,
 'base_bridge_signer':base,'canister_bridge_signer':canister,
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
 'bsns_decimals':state['bsns_decimals'],'bsns_bridge':state['bsns_bridge'],'rpc_provider_urls_sha256':actual_rpc_digest,
 'public_config':public_config}
mode=sys.argv[4]
if mode=='capture':
  target=Path(sys.argv[5]); tmp=Path(str(target)+'.tmp'); tmp.write_text(json.dumps(out,sort_keys=True,separators=(',',':'))+'\n'); tmp.replace(target)
else:
  old=json.load(open(sys.argv[2]))
  snapshot_probes=[]
  for observation in json.load(open(root/'provider-observations.json')):
    if 'snapshot_probe' in observation:
      probe=observation['snapshot_probe']; snapshot_probes.append((num(probe.get('number')),str(probe.get('hash','')).lower()))
  expected=(old['finalized_head_block_number'],old['finalized_head_block_hash'].lower())
  if snapshot_probes.count(expected)<2: raise SystemExit('snapshot Finalized block is no longer canonical')
  if out['finalized_head_block_number'] < old['finalized_head_block_number']: raise SystemExit('latest Finalized head is older than the captured snapshot')
  comparable=lambda x:{k:v for k,v in x.items() if k not in ('observed_at_unix','finalized_head_block_number','finalized_head_block_hash','agreeing_providers')}
  if comparable(out)!=comparable(old): raise SystemExit('live state differs from the captured snapshot')
PY
