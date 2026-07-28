#!/usr/bin/env bash
# Submit exactly one reviewed SNS generic-function proposal and persist its checkpoint.
set -euo pipefail

PHASE="${1:?usage: production-activation-proposal.sh PHASE BUNDLE MANIFEST_SHA256 OUTPUT IDENTITY NEURON_SUBACCOUNT PROPOSER_PRINCIPAL}"
BUNDLE="${2:?missing bundle}"
MANIFEST_SHA256="${3:?missing Gate B manifest hash}"
OUTPUT="${4:?missing output path}"
IDENTITY="${5:?missing ICP identity name}"
NEURON_SUBACCOUNT="${6:?missing SNS neuron subaccount}"
PROPOSER_PRINCIPAL="${7:?missing proposer principal}"

[[ "$PHASE" == schedule || "$PHASE" == execute ]] || { echo "invalid activation phase" >&2; exit 1; }
[[ -d "$BUNDLE" && -f "$BUNDLE/release-manifest.json" && -f "$BUNDLE/profile.json" ]] || {
  echo "activation bundle is incomplete" >&2; exit 1;
}
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || { echo "activation submission output already exists" >&2; exit 1; }
command -v icp >/dev/null || { echo "icp is required" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

# Reserve the checkpoint before proposal submission. If submission is interrupted,
# the empty marker intentionally blocks an unsafe duplicate attempt.
python3 - "$OUTPUT" <<'PY'
import os,sys
target=sys.argv[1]; parent=os.path.dirname(os.path.abspath(target)) or '.'
if not os.path.isdir(parent): raise SystemExit('activation submission parent directory does not exist')
fd=os.open(target,os.O_WRONLY|os.O_CREAT|os.O_EXCL,0o600)
os.fsync(fd); os.close(fd)
PY

python3 - "$PHASE" "$BUNDLE" "$MANIFEST_SHA256" "$OUTPUT" "$IDENTITY" "$NEURON_SUBACCOUNT" "$PROPOSER_PRINCIPAL" <<'PY'
import hashlib,json,os,re,subprocess,sys,time
from pathlib import Path

phase,bundle,manifest_hash,output,identity,subaccount,proposer=sys.argv[1:]
root=Path(bundle); target=Path(output)
profile=json.load(open(root/'profile.json',encoding='utf-8'))
manifest=json.load(open(root/'release-manifest.json',encoding='utf-8'))
governance='74ncn-fqaaa-aaaaq-aaasa-cai'
method='schedule_activation' if phase=='schedule' else 'execute_activation'
payload=bytes.fromhex('4449444c0000')
if not re.fullmatch(r'[A-Za-z0-9_.-]+',identity): raise SystemExit('invalid ICP identity name')
if not re.fullmatch(r'[0-9a-fA-F]{64}',subaccount): raise SystemExit('neuron subaccount must be 32-byte hex')
if not re.fullmatch(r'[a-z0-9-]{5,80}',proposer): raise SystemExit('invalid proposer principal')
if not re.fullmatch(r'[0-9a-fA-F]{64}',manifest_hash): raise SystemExit('invalid Gate B manifest hash')

principal_result=subprocess.run(
 ['icp','identity','principal','--identity',identity],text=True,capture_output=True,check=False)
if principal_result.returncode!=0: raise SystemExit('failed to resolve the SNS proposer identity')
resolved_principal=principal_result.stdout.strip()
if resolved_principal!=proposer: raise SystemExit('SNS identity principal differs from the approved proposer principal')

def call(method_name,arg):
 command=['icp','canister','call',governance,method_name,arg,'-n','ic','--identity',identity,'--json']
 result=subprocess.run(command,text=True,capture_output=True,check=False)
 if result.returncode!=0: raise SystemExit(f'{method_name} failed without an accepted checkpoint')
 return command,result.stdout,result.stderr
def walk(value,key):
 out=[]
 if isinstance(value,dict):
  for name,item in value.items():
   if name==key: out.append(item)
   out.extend(walk(item,key))
 elif isinstance(value,list):
  for item in value: out.extend(walk(item,key))
 return out
def scalar(value):
 while isinstance(value,list) and len(value)==1: value=value[0]
 if isinstance(value,dict) and len(value)==1 and next(iter(value)) in {'Some','Ok'}:
  return scalar(next(iter(value.values())))
 return value

registry_command,registry_stdout,registry_stderr=call('list_nervous_system_functions','()')
try: registry=json.loads(registry_stdout)
except json.JSONDecodeError: raise SystemExit('SNS function registry response is not JSON')
functions=[]
for candidate in walk(registry,'functions'):
 if isinstance(candidate,list): functions.extend(candidate)
matches=[]
for function in functions:
 if not isinstance(function,dict): continue
 identifiers=walk(function,'id'); targets=walk(function,'target_canister_id'); methods=walk(function,'target_method_name')
 try:
  identifier=int(scalar(identifiers[0]))
  target_value=str(scalar(targets[0])); method_value=str(scalar(methods[0]))
 except (IndexError,TypeError,ValueError): continue
 if target_value==profile['bridge_canister_id'] and method_value==method: matches.append(identifier)
if len(matches)!=1: raise SystemExit('SNS function registry has no unique exact activation function')
function_id=matches[0]

def blob_literal(raw): return 'blob "'+''.join(f'\\{byte:02x}' for byte in raw)+'"'
title='Schedule KINIC Bridge activation' if phase=='schedule' else 'Execute KINIC Bridge activation'
summary=('Schedule the reviewed 72-hour Timelock activation while all asset flows remain paused.'
 if phase=='schedule' else 'Execute the reviewed activation and begin production asset acceptance.')
argument=(f'(record {{ subaccount = {blob_literal(bytes.fromhex(subaccount))}; command = opt variant {{ '
 f'MakeProposal = record {{ url = ""; title = "{title}"; summary = "{summary}"; action = opt variant {{ '
 f'ExecuteGenericNervousSystemFunction = record {{ function_id = {function_id} : nat64; payload = {blob_literal(payload)} }} }} }} }} }})')
submitted_at_unix=int(time.time())
submit_command,stdout,stderr=call('manage_neuron',argument)
try: response=json.loads(stdout)
except json.JSONDecodeError: raise SystemExit('SNS proposal response is not JSON')
proposal_ids=[]
for candidate in walk(response,'proposal_id'):
 for value in walk(candidate,'id') if isinstance(candidate,(dict,list)) else [candidate]:
  try: proposal_ids.append(int(scalar(value)))
  except (TypeError,ValueError): pass
proposal_ids=sorted(set(proposal_ids))
if len(proposal_ids)!=1 or proposal_ids[0]<=0: raise SystemExit('SNS response has no unique proposal ID')
evidence={
 'schema_version':3,'phase':phase,'release_id':manifest['release_id'],'source_revision':manifest['source_revision'],
 'source_tree_sha256':manifest['source_tree_sha256'],'gate_b_manifest_sha256':manifest_hash.lower(),
 'governance_canister_id':governance,'bridge_canister_id':profile['bridge_canister_id'],'function_id':function_id,
 'target_method_name':method,'payload_hex':payload.hex(),'payload_sha256':hashlib.sha256(payload).hexdigest(),
 'proposer_principal':resolved_principal,'neuron_subaccount':subaccount.lower(),
 'proposal_id':proposal_ids[0],'submitted_at_unix':submitted_at_unix,
 'registry_response_sha256':hashlib.sha256(registry_stdout.encode()).hexdigest(),
 'proposal_response_hex':stdout.encode().hex(),'proposal_response_sha256':hashlib.sha256(stdout.encode()).hexdigest(),
 'registry_command_argv':registry_command,'proposal_command_argv':[value if value!=argument else '<fixed-candid-payload>' for value in submit_command],
}
tmp=Path(str(target)+f'.tmp.{os.getpid()}')
tmp.write_text(json.dumps(evidence,sort_keys=True,separators=(',',':'))+'\n',encoding='utf-8')
os.replace(tmp,target)
print(f"proposal_submitted phase={phase} proposal_id={proposal_ids[0]} submission={target}")
PY
