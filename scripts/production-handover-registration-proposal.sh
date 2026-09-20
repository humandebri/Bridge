#!/usr/bin/env bash
# Submit the reviewed RegisterDappCanisters proposal only while recovery control remains available.
set -euo pipefail

BUNDLE="${1:?usage: production-handover-registration-proposal.sh BUNDLE MANIFEST_SHA256 OUTPUT IDENTITY NEURON_SUBACCOUNT PROPOSER_PRINCIPAL REVIEWED_HANDOVER_JSON}"
MANIFEST_SHA256="${2:?missing Gate B manifest hash}"
OUTPUT="${3:?missing output path}"
IDENTITY="${4:?missing ICP identity name}"
NEURON_SUBACCOUNT="${5:?missing SNS neuron subaccount}"
PROPOSER_PRINCIPAL="${6:?missing proposer principal}"
REVIEWED="${7:?missing reviewed handover JSON}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$ROOT/scripts/production-validation.sh"

: "${BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:?missing operational config seal receipt}"
: "${BRIDGE_CONTROLLER_SCHEDULE_RECEIPT:?missing controller schedule receipt}"
: "${BRIDGE_CONTROLLER_ACTIVATION_RECEIPT:?missing controller execute receipt}"

[[ -d "$BUNDLE" && -f "$BUNDLE/release-manifest.json" && -f "$BUNDLE/profile.json" ]] || {
  echo "handover bundle is incomplete" >&2; exit 1;
}
[[ -f "$REVIEWED" && ! -L "$REVIEWED" ]] || { echo "reviewed handover is missing or unsafe" >&2; exit 1; }
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || { echo "registration submission output already exists" >&2; exit 1; }
[[ ! -e "$OUTPUT.response.json" && ! -L "$OUTPUT.response.json" ]] || { echo "proposal response journal already exists" >&2; exit 1; }
for tool in icp node python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-handover-registration.XXXXXX")"
trap 'chmod -R u+w "$TMP" 2>/dev/null || true; rm -rf "$TMP"' EXIT
mkdir -m 700 "$TMP/release-bundle"
production_freeze_bundle "$BUNDLE" "$TMP/release-bundle"
production_freeze_receipt "$REVIEWED" "$TMP/reviewed.json" "reviewed handover"
production_freeze_receipt "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$TMP/seal.json" "seal receipt"
production_freeze_receipt "$BRIDGE_CONTROLLER_SCHEDULE_RECEIPT" "$TMP/schedule.json" "schedule receipt"
production_freeze_receipt "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT" "$TMP/execute.json" "execute receipt"
BUNDLE="$TMP/release-bundle"
REVIEWED="$TMP/reviewed.json"
BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT="$TMP/seal.json"
BRIDGE_CONTROLLER_SCHEDULE_RECEIPT="$TMP/schedule.json"
BRIDGE_CONTROLLER_ACTIVATION_RECEIPT="$TMP/execute.json"
export BRIDGE_HANDOVER_VALIDATOR_BIN="$TMP/bridge-profile"
BRIDGE_CURRENT_MODULE_SHA256="$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["wasm_sha256"])' "$REVIEWED")"
export BRIDGE_CURRENT_MODULE_SHA256 BRIDGE_PRODUCTION_INSTALLER_IDENTITY=production
CARGO_TARGET_DIR="$TMP/profile-target" cargo build --locked --quiet --release \
  --manifest-path "$ROOT/Cargo.toml" -p bridge-profile
BRIDGE_HANDOVER_VALIDATOR_BIN="$TMP/profile-target/release/bridge-profile"
production_require_clean_source "$ROOT"
REVISION="$(git -C "$ROOT" rev-parse HEAD)"
TREE="$(git -C "$ROOT" archive HEAD | shasum -a 256 | awk '{print tolower($1)}')"
production_run_proof_gate "$ROOT" "$REVISION" "$TREE"
"$BRIDGE_HANDOVER_VALIDATOR_BIN" verify-production-current-state "$BUNDLE/profile.json" \
  "$PROPOSER_PRINCIPAL" "$BRIDGE_CURRENT_MODULE_SHA256" joint

python3 - "$BUNDLE" "$MANIFEST_SHA256" "$OUTPUT" "$IDENTITY" "$NEURON_SUBACCOUNT" "$PROPOSER_PRINCIPAL" "$REVIEWED" "$ROOT" <<'PY'
import hashlib,json,os,re,subprocess,sys,tempfile,time
from pathlib import Path

bundle,manifest_hash,output,identity,subaccount,proposer,reviewed_path,repo=sys.argv[1:]
bundle=Path(bundle); output=Path(output); reviewed_path=Path(reviewed_path); repo=Path(repo)
governance='74ncn-fqaaa-aaaaq-aaasa-cai'; root='7jkta-eyaaa-aaaaq-aaarq-cai'
manifest=json.loads((bundle/'release-manifest.json').read_text()); profile=json.loads((bundle/'profile.json').read_text())
reviewed_bytes=reviewed_path.read_bytes(); reviewed=json.loads(reviewed_bytes)
if not re.fullmatch(r'[0-9a-fA-F]{64}',manifest_hash): raise SystemExit('invalid Gate B manifest hash')
if hashlib.sha256((bundle/'release-manifest.json').read_bytes()).hexdigest()!=manifest_hash.lower(): raise SystemExit('Gate B manifest bytes differ from approval')
if not re.fullmatch(r'[A-Za-z0-9_.-]+',identity): raise SystemExit('invalid ICP identity name')
if not re.fullmatch(r'[0-9a-fA-F]{64}',subaccount): raise SystemExit('neuron subaccount must be 32-byte hex')
if not re.fullmatch(r'[a-z0-9-]{5,80}',proposer): raise SystemExit('invalid proposer principal')
if reviewed.get('schema_version')!=1 or reviewed.get('governance_canister_id')!=governance or reviewed.get('root_canister_id')!=root: raise SystemExit('reviewed handover governance domain differs')
if reviewed.get('bridge_canister_id')!=profile.get('bridge_canister_id') or reviewed.get('wasm_sha256')!=profile.get('bridge_canister_wasm_sha256'): raise SystemExit('reviewed handover target differs')
if reviewed.get('already_registered') is not False or not isinstance(reviewed.get('registration_proposal'),str): raise SystemExit('Bridge must be unregistered before proposal submission')
expected=subprocess.run(['node',str(repo/'tools/sns-proposal/handover.mjs'),'registration-payload',profile['bridge_canister_id']],text=True,capture_output=True,check=False)
if expected.returncode!=0 or reviewed['registration_proposal']!=expected.stdout.rstrip('\n'): raise SystemExit('reviewed handover is not the fixed single-Bridge RegisterDappCanisters action')

resolved=subprocess.run(['icp','identity','principal','--identity',identity],text=True,capture_output=True,check=False)
if resolved.returncode!=0 or resolved.stdout.strip()!=proposer: raise SystemExit('SNS proposer identity differs from approval')
status=subprocess.run(['icp','canister','status','bridge-canister','-e','production','--identity',identity,'--json'],text=True,capture_output=True,check=False)
if status.returncode!=0: raise SystemExit('failed to read Bridge controller state')
try: status_json=json.loads(status.stdout)
except json.JSONDecodeError: raise SystemExit('Bridge status is not JSON')
def values(item,key):
 out=[]
 if isinstance(item,dict):
  for k,v in item.items():
   if k==key: out.append(v)
   out.extend(values(v,key))
 elif isinstance(item,list):
  for v in item: out.extend(values(v,key))
 return out
controllers=values(status_json,'controllers')
controllers=controllers[0] if len(controllers)==1 and isinstance(controllers[0],list) else []
if len(controllers)!=2 or set(map(str,controllers))!={proposer,root}: raise SystemExit('live Bridge controllers are not the exact reviewed co-controller set')
root_call=['icp','canister','call',root,'list_sns_canisters','(record {})','-n','ic','--query','--identity',identity,'--json']
root_result=subprocess.run(root_call,text=True,capture_output=True,check=False)
if root_result.returncode!=0: raise SystemExit('failed to read SNS Root registration')
with tempfile.TemporaryDirectory(prefix='bridge-root-registration.') as work:
 p=Path(work)/'root.json'; p.write_text(root_result.stdout)
 decoded=subprocess.run(['node',str(repo/'tools/sns-proposal/handover.mjs'),'decode-root',str(p)],text=True,capture_output=True,check=False)
 if decoded.returncode!=0: raise SystemExit('SNS Root response could not be decoded')
 dapps=json.loads(decoded.stdout)
if profile['bridge_canister_id'] in dapps: raise SystemExit('Bridge is already registered; do not submit a duplicate proposal')

parent=output.parent.resolve()
if not parent.is_dir(): raise SystemExit('submission parent directory does not exist')
reserve_fd=os.open(output,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
response_path=Path(str(output)+'.response.json')
response_fd=os.open(response_path,os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
os.fsync(reserve_fd); os.close(reserve_fd)
parent_fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW); os.fsync(parent_fd); os.close(parent_fd)
proposal=reviewed['registration_proposal']
blob='blob "'+''.join(f'\\{b:02x}' for b in bytes.fromhex(subaccount))+'"'
argument=f'(record {{ subaccount = {blob}; command = opt variant {{ MakeProposal = {proposal} }} }})'
command=['icp','canister','call',governance,'manage_neuron',argument,'-n','ic','--identity',identity,'--json']
submitted_at=int(time.time())
result=subprocess.run(command,text=True,capture_output=True,check=False)
journal={'command_argv':[v if v!=argument else '<fixed-candid-payload>' for v in command],'exit_code':result.returncode,
 'stdout':result.stdout,'stderr':result.stderr,'observed_at_unix':int(time.time()),
 'stdout_sha256':hashlib.sha256(result.stdout.encode()).hexdigest()}
data=(json.dumps(journal,sort_keys=True,separators=(',',':'))+'\n').encode()
view=memoryview(data)
while view:
 written=os.write(response_fd,view)
 if written<=0: raise SystemExit('short write while persisting registration response journal')
 view=view[written:]
os.fsync(response_fd); os.close(response_fd)
if result.returncode!=0: raise SystemExit('registration proposal result is uncertain; preserve journal and do not resubmit')
try: response_json=json.loads(result.stdout)
except json.JSONDecodeError: raise SystemExit('registration proposal response is not JSON; preserve journal and do not resubmit')
with tempfile.TemporaryDirectory(prefix='bridge-registration-response.') as work:
 p=Path(work)/'response.json'; p.write_text(result.stdout)
 decoded=subprocess.run(['node',str(repo/'tools/sns-proposal/handover.mjs'),'decode-response',str(p)],text=True,capture_output=True,check=False)
 if decoded.returncode!=0: raise SystemExit('registration proposal ID is unavailable; preserve journal and do not resubmit')
 proposal_id=int(decoded.stdout.strip())
evidence={'schema_version':1,'kind':'sns-dapp-registration-submission','release_id':manifest['release_id'],
 'source_revision':manifest['source_revision'],'source_tree_sha256':manifest['source_tree_sha256'],
 'gate_b_manifest_sha256':manifest_hash.lower(),'governance_canister_id':governance,'sns_root_canister_id':root,
 'bridge_canister_id':profile['bridge_canister_id'],'proposer_principal':proposer,'neuron_subaccount':subaccount.lower(),
 'proposal_id':proposal_id,'submitted_at_unix':submitted_at,'proposal_sha256':hashlib.sha256(proposal.encode()).hexdigest(),
 'current_module_sha256':reviewed['wasm_sha256'],'reviewed_handover_sha256':hashlib.sha256(reviewed_bytes).hexdigest(),
 'root_query_response_hex':root_result.stdout.encode().hex(),'root_query_response_sha256':hashlib.sha256(root_result.stdout.encode()).hexdigest(),
 'proposal_response_hex':result.stdout.encode().hex(),'proposal_response_sha256':hashlib.sha256(result.stdout.encode()).hexdigest(),
 'root_command_argv':root_call,'proposal_command_argv':[v if v!=argument else '<fixed-candid-payload>' for v in command]}
payload=(json.dumps(evidence,sort_keys=True,separators=(',',':'))+'\n').encode()
fd,tmp=tempfile.mkstemp(prefix='.handover-registration.',dir=parent)
try:
 os.fchmod(fd,0o400)
 view=memoryview(payload)
 while view:
  written=os.write(fd,view)
  if written<=0: raise SystemExit('short write while persisting registration submission')
  view=view[written:]
 os.fsync(fd)
finally: os.close(fd)
os.replace(tmp,output); parent_fd=os.open(parent,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW); os.fsync(parent_fd); os.close(parent_fd)
print(f'registration_proposal_submitted proposal_id={proposal_id} submission={output}')
PY
