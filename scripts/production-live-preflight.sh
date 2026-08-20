#!/usr/bin/env bash
# Verify the staging monitor rehearsal against its independently bound Custom RPC URLs.
set -euo pipefail

[[ "${1:-}" == verify-monitor-drill && "$#" == 2 ]] || {
  echo "usage: production-live-preflight.sh verify-monitor-drill BUNDLE" >&2
  exit 2
}
BUNDLE="$2"
DRILL="$BUNDLE/monitor-drill.json"
[[ -f "$DRILL" ]] || { echo "monitor drill evidence is missing" >&2; exit 1; }
for tool in cast python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done
: "${BRIDGE_MONITOR_RPC_URL_1:?missing BRIDGE_MONITOR_RPC_URL_1}"
: "${BRIDGE_MONITOR_RPC_URL_2:?missing BRIDGE_MONITOR_RPC_URL_2}"
: "${BRIDGE_MONITOR_RPC_URL_3:?missing BRIDGE_MONITOR_RPC_URL_3}"
python3 - "$DRILL" "$BRIDGE_MONITOR_RPC_URL_1" "$BRIDGE_MONITOR_RPC_URL_2" "$BRIDGE_MONITOR_RPC_URL_3" <<'PY'
import hashlib,json,re,subprocess,sys
from urllib.parse import urlsplit

drill=json.load(open(sys.argv[1],encoding='utf-8')); providers=sys.argv[2:]
if len(set(providers))!=3: raise SystemExit('monitor drill requires exactly three distinct Base RPC providers')
for rpc in providers:
 p=urlsplit(rpc)
 if p.scheme!='https' or not p.hostname or p.username or p.password or p.query or p.fragment:
  raise SystemExit('monitor drill RPC providers must be credential-free HTTPS URLs')
digest=hashlib.sha256(json.dumps(providers,separators=(',',':')).encode()).hexdigest()
if digest.lower()!=str(drill['rpc_provider_urls_sha256']).lower():
 raise SystemExit('monitor drill RPC provider digest differs from the rehearsal binding')

def run(args): return subprocess.check_output(args,text=True,stderr=subprocess.DEVNULL).strip()
def number(value):
 if isinstance(value,int): return value
 value=str(value); return int(value,16) if value.startswith('0x') else int(value)
def finalized_block(rpc): return json.loads(run(['cast','block','finalized','--rpc-url',rpc,'--json']))
def verify_all_provider_chain_ids(providers,expected):
 for index,rpc in enumerate(providers):
  try: chain_id=int(run(['cast','chain-id','--rpc-url',rpc]))
  except (OSError,subprocess.CalledProcessError,ValueError):
   raise SystemExit(f'monitor drill RPC provider {index} chain ID check failed')
  if chain_id!=expected:
   raise SystemExit(f'monitor drill RPC provider {index} returned chain ID {chain_id}; expected {expected}')
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
 else: raise ValueError('unsupported canonical probe')

heads=[]
verify_all_provider_chain_ids(providers,drill['base_chain_id'])
for index,rpc in enumerate(providers):
 try:
  value=finalized_block(rpc); heads.append((index,number(value['number']),str(value['hash']).lower()))
 except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError): pass
if not heads: raise SystemExit('monitor drill has no usable Finalized provider')
pairs=[(height,block_hash) for _,height,block_hash in heads]
winner=max(set(pairs),key=pairs.count)
eligible={index for index,height,block_hash in heads if (height,block_hash)==winner}
if len(eligible)<2: raise SystemExit('monitor drill Finalized head has no 2-of-3 agreement')

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
   if (receipt_number==action['block_number'] and status==1 and receipt_hash==action['block_hash'].lower()
       and target==expected_address and calldata==action['calldata_hex'].lower()
       and winner[0]>=receipt_number and log_match):
    signature='getMinDelay()' if action['kind']=='CancelTimelock' else 'bridgeSnapshot()'
    canonical_probe(rpc,expected_address,signature,receipt_hash,receipt_number)
    matches+=1
  except (OSError,subprocess.CalledProcessError,ValueError,KeyError,json.JSONDecodeError): pass
 if matches<2: raise SystemExit(f"monitor drill action {action['kind']} lacks 2-of-3 canonical Finalized agreement")
print('monitor_drill_base=pass')
PY
