#!/usr/bin/env bash
# Submit exactly one reviewed SNS generic-function proposal and persist its checkpoint.
set -euo pipefail

PHASE="${1:?usage: production-activation-proposal.sh PHASE BUNDLE MANIFEST_SHA256 OUTPUT IDENTITY NEURON_SUBACCOUNT PROPOSER_PRINCIPAL PREVIOUS_OPERATION_ID REVIEWED_PROPOSALS_JSON}"
BUNDLE="${2:?missing bundle}"
MANIFEST_SHA256="${3:?missing Gate B manifest hash}"
OUTPUT="${4:?missing output path}"
IDENTITY="${5:?missing ICP identity name}"
NEURON_SUBACCOUNT="${6:?missing SNS neuron subaccount}"
PROPOSER_PRINCIPAL="${7:?missing proposer principal}"
PREVIOUS_OPERATION_ID="${8:?missing previous confirmed governance operation ID}"
REVIEWED_PROPOSALS="${9:?missing reviewed proposal preparation JSON}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

[[ "$PHASE" == schedule || "$PHASE" == execute ]] || { echo "invalid activation phase" >&2; exit 1; }
[[ -d "$BUNDLE" && -f "$BUNDLE/release-manifest.json" && -f "$BUNDLE/profile.json" ]] || {
  echo "activation bundle is incomplete" >&2; exit 1;
}
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || { echo "activation submission output already exists" >&2; exit 1; }
[[ ! -e "$OUTPUT.response.json" && ! -L "$OUTPUT.response.json" ]] || { echo "proposal response journal already exists" >&2; exit 1; }
command -v icp >/dev/null || { echo "icp is required" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 1; }

python3 - "$PHASE" "$BUNDLE" "$MANIFEST_SHA256" "$OUTPUT" "$IDENTITY" "$NEURON_SUBACCOUNT" "$PROPOSER_PRINCIPAL" "$PREVIOUS_OPERATION_ID" "$ROOT" "$REVIEWED_PROPOSALS" <<'PY'
import hashlib,json,os,re,shutil,subprocess,sys,tempfile,time
from pathlib import Path

phase,bundle,manifest_hash,output,identity,subaccount,proposer,previous,repo,reviewed_path=sys.argv[1:]
node=shutil.which('node')
expected_node='v'+(Path(repo)/'.node-version').read_text().strip()
if node is None: raise SystemExit('Node '+expected_node+' is required on PATH')
node_version=subprocess.run([node,'--version'],text=True,capture_output=True,check=False)
if node_version.returncode!=0 or node_version.stdout.strip()!=expected_node:
 raise SystemExit('Node on PATH must match '+expected_node)
root=Path(bundle); target=Path(output)
profile=json.load(open(root/'profile.json',encoding='utf-8'))
manifest=json.load(open(root/'release-manifest.json',encoding='utf-8'))
governance='74ncn-fqaaa-aaaaq-aaasa-cai'
method=f'sns_{phase}_activation'
validator=f'validate_{method}'
if not re.fullmatch(r'[A-Za-z0-9_.-]+',identity): raise SystemExit('invalid ICP identity name')
if not re.fullmatch(r'[0-9a-fA-F]{64}',subaccount): raise SystemExit('neuron subaccount must be 32-byte hex')
if not re.fullmatch(r'[a-z0-9-]{5,80}',proposer): raise SystemExit('invalid proposer principal')
if not re.fullmatch(r'[0-9a-fA-F]{64}',manifest_hash): raise SystemExit('invalid Gate B manifest hash')
if hashlib.sha256((root/'release-manifest.json').read_bytes()).hexdigest()!=manifest_hash.lower(): raise SystemExit('Gate B manifest bytes differ from approval')

principal_result=subprocess.run(
 ['icp','identity','principal','--identity',identity],text=True,capture_output=True,check=False)
if principal_result.returncode!=0: raise SystemExit('failed to resolve the SNS proposer identity')
resolved_principal=principal_result.stdout.strip()
if resolved_principal!=proposer: raise SystemExit('SNS identity principal differs from the approved proposer principal')

def call(method_name,arg):
 command=['icp','canister','call',governance,method_name,arg,'-n','ic','--identity',identity,'--json']
 if method_name=='list_nervous_system_functions': command.append('--query')
 result=subprocess.run(command,text=True,capture_output=True,check=False)
 if method_name=='manage_neuron':
  captured={'command_argv':command,'exit_code':result.returncode,'stdout':result.stdout,'stderr':result.stderr,
   'observed_at_unix':int(time.time()),'stdout_sha256':hashlib.sha256(result.stdout.encode()).hexdigest()}
  data=(json.dumps(captured,sort_keys=True)+'\n').encode()
  view=memoryview(data)
  while view:
   written=os.write(response_fd,view)
   if written<=0: raise SystemExit('short write to proposal response journal; do not resubmit')
   view=view[written:]
  os.fsync(response_fd); os.close(response_fd)
 if result.returncode!=0: raise SystemExit(f'{method_name} failed without an accepted checkpoint; preserve the response journal and do not resubmit')
 return command,result.stdout,result.stderr

registry_command,registry_stdout,registry_stderr=call('list_nervous_system_functions','()')
try: registry=json.loads(registry_stdout)
except json.JSONDecodeError: raise SystemExit('SNS function registry response is not JSON')
with tempfile.TemporaryDirectory(prefix='bridge-sns-prepare.') as work:
 registry_path=Path(work)/'registry.json'; prepared_path=Path(work)/'prepared.json'
 registry_path.write_text(registry_stdout,encoding='utf-8')
 prepared=subprocess.run([node,str(Path(repo)/'tools/sns-proposal/prepare.mjs'),str(registry_path),profile['bridge_canister_id'],previous,str(prepared_path)],
  text=True,capture_output=True,check=False)
 if prepared.returncode!=0: raise SystemExit('typed SNS registry/payload validation failed: '+prepared.stderr)
 proposals=json.loads(prepared_path.read_text())['proposals']
 selected=next(item for item in proposals if item['phase']==phase)
 if selected['registration_proposal'] is not None: raise SystemExit('SNS activation function is not registered')
 function_id=int(selected['function_id']); payload=bytes.fromhex(selected['payload_hex'])
 reviewed=json.loads(Path(reviewed_path).read_text())
 if reviewed.get('schema_version')!=1 or reviewed.get('governance_canister_id')!=governance: raise SystemExit('reviewed proposal preparation domain differs')
 matches=[item for item in reviewed.get('proposals',[]) if item.get('phase')==phase]
 if len(matches)!=1: raise SystemExit('reviewed proposal phase is not unique')
 for key in ['function_id','target_canister_id','target_method_name','validator_canister_id','validator_method_name','previous_governance_operation_id','payload_hex','payload_sha256','execution_proposal']:
  if matches[0].get(key)!=selected[key]: raise SystemExit('live proposal differs from the reviewed preparation: '+key)
 reviewed_proposal=matches[0]['execution_proposal']

# All reversible identity and registry checks are complete. Reserve the durable
# checkpoint immediately before the irreversible proposal submission.
parent=target.parent.resolve()
if not parent.is_dir(): raise SystemExit('activation submission parent directory does not exist')
reserve_fd=os.open(target,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
response_fd=os.open(str(target)+'.response.json',os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
try: os.fsync(reserve_fd)
finally: os.close(reserve_fd)
parent_fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
try: os.fsync(parent_fd)
finally: os.close(parent_fd)

def blob_literal(raw): return 'blob "'+''.join(f'\\{byte:02x}' for byte in raw)+'"'
argument=(f'(record {{ subaccount = {blob_literal(bytes.fromhex(subaccount))}; command = opt variant {{ '
 f'MakeProposal = {reviewed_proposal} }} }})')
submitted_at_unix=int(time.time())
submit_command,stdout,stderr=call('manage_neuron',argument)
try: response=json.loads(stdout)
except json.JSONDecodeError: raise SystemExit('SNS proposal response is not JSON')
with tempfile.TemporaryDirectory(prefix='bridge-sns-response.') as work:
 response_path=Path(work)/'response.json'; response_path.write_text(stdout,encoding='utf-8')
 decoded=subprocess.run([node,str(Path(repo)/'tools/sns-proposal/prepare.mjs'),'decode-response',str(response_path)],
  text=True,capture_output=True,check=False)
 if decoded.returncode!=0: raise SystemExit('SNS proposal response could not be decoded; checkpoint retained, do not resubmit')
 proposal_ids=[int(decoded.stdout.strip())]
evidence={
 'schema_version':4,'phase':phase,'release_id':manifest['release_id'],'source_revision':manifest['source_revision'],
 'source_tree_sha256':manifest['source_tree_sha256'],'gate_b_manifest_sha256':manifest_hash.lower(),
 'governance_canister_id':governance,'bridge_canister_id':profile['bridge_canister_id'],'function_id':function_id,
 'target_method_name':method,'validator_canister_id':profile['bridge_canister_id'],
 'validator_method_name':validator,'previous_governance_operation_id':int(previous),'payload_hex':payload.hex(),'payload_sha256':hashlib.sha256(payload).hexdigest(),
 'proposer_principal':resolved_principal,'neuron_subaccount':subaccount.lower(),
 'proposal_id':proposal_ids[0],'submitted_at_unix':submitted_at_unix,
 'registry_response_sha256':hashlib.sha256(registry_stdout.encode()).hexdigest(),
 'proposal_response_hex':stdout.encode().hex(),'proposal_response_sha256':hashlib.sha256(stdout.encode()).hexdigest(),
 'registry_command_argv':registry_command,'proposal_command_argv':[value if value!=argument else '<fixed-candid-payload>' for value in submit_command],
}
payload_bytes=(json.dumps(evidence,sort_keys=True,separators=(',',':'))+'\n').encode()
tmp_fd,tmp_name=tempfile.mkstemp(prefix='.activation-evidence.',dir=parent)
try:
 view=memoryview(payload_bytes)
 while view:
  written=os.write(tmp_fd,view)
  if written<=0: raise SystemExit('short write while saving activation evidence')
  view=view[written:]
 os.fsync(tmp_fd)
finally: os.close(tmp_fd)
os.replace(tmp_name,target)
parent_fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW)
try: os.fsync(parent_fd)
finally: os.close(parent_fd)
print(f"proposal_submitted phase={phase} proposal_id={proposal_ids[0]} submission={target}")
PY
