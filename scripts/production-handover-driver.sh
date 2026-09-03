#!/usr/bin/env bash
# Atomically transfer the active production Bridge Canister to the KINIC SNS Root only.
set -euo pipefail
[[ $# -eq 0 ]] || {
  echo "usage: BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT=seal-receipt.json BRIDGE_CONTROLLER_SCHEDULE_RECEIPT=schedule-receipt.json BRIDGE_CONTROLLER_ACTIVATION_RECEIPT=execute-receipt.json $0" >&2
  exit 2
}

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_B_MANIFEST_SHA256:?missing Gate B approval}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT:?missing operational config seal receipt}"
: "${BRIDGE_CONTROLLER_SCHEDULE_RECEIPT:?missing controller schedule receipt}"
: "${BRIDGE_CONTROLLER_ACTIVATION_RECEIPT:?missing controller execute receipt}"
: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP CLI identity}"
: "${BRIDGE_HANDOVER_EVIDENCE_FILE:?missing handover evidence output path}"
: "${BRIDGE_HANDOVER_CONFIRMATION:?set BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY}"
[[ "$BRIDGE_HANDOVER_CONFIRMATION" == TRANSFER_TO_KINIC_SNS_ROOT_ONLY ]] || {
  echo "controller handover requires the exact confirmation phrase" >&2; exit 1;
}
[[ ! -e "$BRIDGE_HANDOVER_EVIDENCE_FILE" && ! -L "$BRIDGE_HANDOVER_EVIDENCE_FILE" ]] || {
  echo "handover evidence output already exists or is a symlink" >&2; exit 1;
}
for tool in icp python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-handover.XXXXXX")"
cleanup_handover_tmp() {
  chmod -R u+w "$TMP" 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup_handover_tmp EXIT
FROZEN_BUNDLE="$TMP/release-bundle"
mkdir -m 700 "$FROZEN_BUNDLE"
production_freeze_bundle "$BRIDGE_RELEASE_BUNDLE" "$FROZEN_BUNDLE"
production_freeze_receipt "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$TMP/seal-receipt.json" "seal receipt"
production_freeze_receipt "$BRIDGE_CONTROLLER_SCHEDULE_RECEIPT" "$TMP/schedule-receipt.json" "schedule receipt"
production_freeze_receipt "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT" "$TMP/execute-receipt.json" "execute receipt"
BRIDGE_RELEASE_BUNDLE="$FROZEN_BUNDLE"
BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT="$TMP/seal-receipt.json"
BRIDGE_CONTROLLER_SCHEDULE_RECEIPT="$TMP/schedule-receipt.json"
BRIDGE_CONTROLLER_ACTIVATION_RECEIPT="$TMP/execute-receipt.json"
BRIDGE_HANDOVER_VALIDATOR_BIN="$TMP/bridge-profile"

production_validate_gate handover "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_B_MANIFEST_SHA256" "" \
  "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$BRIDGE_CONTROLLER_SCHEDULE_RECEIPT" \
  "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT"
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"
read -r CANISTER ROOT CYCLES_FLOOR EXPECTED_WASM < <(python3 -c '
import json,sys
p=json.load(open(sys.argv[1])); print(p["bridge_canister_id"],p["root_canister_id"],p["parameters"]["cycles_floor"],p["bridge_canister_wasm_sha256"])
' "$PROFILE")
[[ "$CANISTER" =~ ^[a-z0-9-]+$ && "$ROOT" == 7jkta-eyaaa-aaaaq-aaarq-cai ]] || {
  echo "handover profile does not bind a production Bridge and the fixed KINIC SNS Root" >&2; exit 1;
}
[[ "$(icp canister status bridge-canister -e production -i --identity "$BRIDGE_ICP_IDENTITY")" == "$CANISTER" ]] || {
  echo "production ICP environment does not map the reviewed Bridge Canister" >&2; exit 1;
}

icp canister call bridge-canister get_bridge_status '()' -e production --json >"$TMP/bridge-status.json"
icp canister call bridge-canister get_production_lifecycle '()' -e production --json >"$TMP/lifecycle.json"
icp canister call bridge-canister get_runtime_binding '()' -e production --json >"$TMP/runtime-binding.json"
icp canister call bridge-canister storage_integrity_check '()' -e production --json >"$TMP/storage-integrity.json"
icp canister call bridge-canister get_activation_status '()' -e production --json >"$TMP/activation-status.json"
icp canister call bridge-canister get_activation_attestation '()' -e production --json >"$TMP/activation-attestation.json"
icp canister status bridge-canister -e production --identity "$BRIDGE_ICP_IDENTITY" --json >"$TMP/canister-status.json"
EXECUTING_PRINCIPAL="$(icp identity principal --identity "$BRIDGE_ICP_IDENTITY")"
python3 - "$TMP/bridge-status.json" "$TMP/canister-status.json" "$EXECUTING_PRINCIPAL" "$CYCLES_FLOOR" "$EXPECTED_WASM" "$TMP/lifecycle.json" "$TMP/runtime-binding.json" "$TMP/storage-integrity.json" "$TMP/activation-status.json" "$TMP/activation-attestation.json" "$BRIDGE_RELEASE_BUNDLE/release-manifest.json" "$BRIDGE_GATE_B_MANIFEST_SHA256" "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" "$BRIDGE_CONTROLLER_SCHEDULE_RECEIPT" "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT" >"$TMP/preflight.json" <<'PY'
import hashlib,json,re,sys
bridge_path,status_path,caller,floor,expected_wasm,lifecycle_path,runtime_path,integrity_path,activation_path,attestation_path,manifest_path,gate_hash,seal_path,schedule_path,execute_path=sys.argv[1:]
bridge=json.load(open(bridge_path)); status=json.load(open(status_path)); floor=int(floor)
def values(value,key):
 out=[]
 if isinstance(value,dict):
  for k,v in value.items():
   if k==key: out.append(v)
   out.extend(values(v,key))
 elif isinstance(value,list):
  for v in value: out.extend(values(v,key))
 return out
def number(value):
 if type(value) is int and value >= 0: return value
 if not isinstance(value,str): raise SystemExit('canister status integer is malformed')
 text=value.strip()
 if re.fullmatch(r'0x[0-9a-fA-F]+',text): return int(text,16)
 if re.fullmatch(r'[0-9]+(?:_[0-9]+)*',text): return int(text.replace('_',''))
 raise SystemExit('canister status integer is malformed')
controllers=values(status,'controllers')
if len(controllers)!=1 or not isinstance(controllers[0],list): raise SystemExit('canister status lacks one controller list')
controllers=controllers[0]
if [str(v) for v in controllers] != [caller]: raise SystemExit('handover requires the production identity as sole controller')
module_values=values(status,'module_hash') or values(status,'module')
if len(module_values)!=1: raise SystemExit('canister status lacks one module hash')
module=str(module_values[0]).strip().strip('"').lower().removeprefix('0x')
if module != expected_wasm.lower().removeprefix('0x'): raise SystemExit('live module does not match the post-Gate-A upgrade lineage')
cycles_values=values(status,'cycles') or values(status,'cycles_balance')
threshold_values=values(status,'freezing_threshold') or values(status,'freezing_threshold_seconds')
burn_values=values(status,'idle_cycles_burned_per_day')
if len(cycles_values)!=1 or len(threshold_values)!=1 or len(burn_values)!=1: raise SystemExit('canister status lacks one cycles/freezing input')
cycles=number(cycles_values[0]); threshold=number(threshold_values[0]); burn=number(burn_values[0])
freeze_required=(burn*threshold+86399)//86400
if cycles < floor or cycles < freeze_required: raise SystemExit('cycles do not satisfy floor and freezing requirement')
def evidence(path,prefix):
 raw=open(path,'rb').read()
 return {prefix+'_response_json_hex':raw.hex(),prefix+'_response_sha256':hashlib.sha256(raw).hexdigest()}
manifest=json.load(open(manifest_path))
digest=lambda path: hashlib.sha256(open(path,'rb').read()).hexdigest()
snapshot={'source_revision':manifest['source_revision'],'source_tree_sha256':manifest['source_tree_sha256'],
          'gate_b_manifest_sha256':gate_hash.lower(),
          'operational_config_seal_receipt_sha256':digest(seal_path),
          'controller_schedule_receipt_sha256':digest(schedule_path),
          'controller_execute_receipt_sha256':digest(execute_path),
          'before_controllers':[caller],'before_module_sha256':module,'cycles_balance':cycles,
          'freezing_threshold_seconds':threshold,'idle_cycles_burned_per_day':burn,
          'required_freezing_cycles':freeze_required}
for path,prefix in [(bridge_path,'before_bridge_status'),(status_path,'before_management_status'),
                    (lifecycle_path,'before_lifecycle'),(runtime_path,'before_runtime_binding'),
                    (integrity_path,'before_storage_integrity'),(activation_path,'before_activation_status'),
                    (attestation_path,'before_activation_attestation')]: snapshot.update(evidence(path,prefix))
print(json.dumps(snapshot,sort_keys=True,separators=(',',':')))
PY

# Prove that the durable evidence target supports create, fsync and atomic
# replacement before changing the controller set.
production_reserve_output "$BRIDGE_HANDOVER_EVIDENCE_FILE" "handover evidence"

# Re-run the complete authenticated active-state verifier after the final
# filesystem preparation. Only the sole-controller/module read below may occur
# between this check and the irreversible settings update.
"$BRIDGE_HANDOVER_VALIDATOR_BIN" verify-production-canister-handover \
  "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
  "$BRIDGE_CONTROLLER_SCHEDULE_RECEIPT" "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT" \
  >/dev/null

# Re-read management state after every pre-send preparation step. This is the
# controller/module snapshot that authorizes the irreversible settings update.
icp canister status bridge-canister -e production --identity "$BRIDGE_ICP_IDENTITY" --json >"$TMP/pre-send-management-status.json"
python3 - "$TMP/preflight.json" "$TMP/pre-send-management-status.json" "$EXECUTING_PRINCIPAL" "$EXPECTED_WASM" >"$TMP/preflight-final.json" <<'PY'
import hashlib,json,sys
preflight_path,status_path,caller,expected_wasm=sys.argv[1:]
value=json.load(open(preflight_path)); status=json.load(open(status_path))
def values(item,key):
 out=[]
 if isinstance(item,dict):
  for k,v in item.items():
   if k==key: out.append(v)
   out.extend(values(v,key))
 elif isinstance(item,list):
  for v in item: out.extend(values(v,key))
 return out
controllers=values(status,'controllers')
controllers=controllers[0] if controllers and isinstance(controllers[0],list) else controllers
if [str(v) for v in controllers] != [caller]: raise SystemExit('pre-send controller set is not the production identity alone')
modules=values(status,'module_hash') or values(status,'module')
if len(modules)!=1: raise SystemExit('pre-send status lacks one module hash')
module=str(modules[0]).strip().strip('"').lower().removeprefix('0x')
if module != expected_wasm.lower().removeprefix('0x'): raise SystemExit('pre-send module differs from the current profile Wasm')
raw=open(status_path,'rb').read()
value.update({'pre_send_controllers':[caller],'pre_send_module_sha256':module,
              'pre_send_management_status_response_json_hex':raw.hex(),
              'pre_send_management_status_response_sha256':hashlib.sha256(raw).hexdigest()})
print(json.dumps(value,sort_keys=True,separators=(',',':')))
PY
mv "$TMP/preflight-final.json" "$TMP/preflight.json"

COMMAND=(icp canister settings update bridge-canister -e production --remove-all-controllers --add-controller "$ROOT" --force --identity "$BRIDGE_ICP_IDENTITY" --debug)
set +e
"${COMMAND[@]}" >"$TMP/response.stdout" 2>"$TMP/response.stderr"
STATUS=$?
set -e
if [[ $STATUS -ne 0 ]]; then
  COMPLETED_AT="$(date +%s)"
  RESPONSE_SHA256="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()+open(sys.argv[2],"rb").read()).hexdigest())' "$TMP/response.stdout" "$TMP/response.stderr")"
  python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$STATUS" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys,tempfile
target,canister,root,caller,status,response_sha,completed,preflight,stdout_path,stderr_path,*argv=sys.argv[1:]
value={'schema_version':3,'stage':'controller_update_uncertain','observed_at_unix':int(completed),
       'bridge_canister_id':canister,'sns_root_canister_id':root,'executing_principal':caller,
       'command_argv':argv,'request_id':'','response_exit_code':int(status),
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,**json.load(open(preflight))}
parent=os.path.dirname(os.path.abspath(target)) or '.'
fd,tmp=tempfile.mkstemp(prefix='.handover-evidence.',dir=parent)
out=os.fdopen(fd,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
  echo "INCIDENT: controller handover result is uncertain; durable checkpoint retained and automatic retry is forbidden" >&2
  exit 1
fi
COMPLETED_AT="$(date +%s)"
RESPONSE_SHA256="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()+open(sys.argv[2],"rb").read()).hexdigest())' "$TMP/response.stdout" "$TMP/response.stderr")"
REQUEST_ID="$(python3 -c '
import re,sys
text=open(sys.argv[1],errors="replace").read()+open(sys.argv[2],errors="replace").read()
matches={value.lower() for value in re.findall(r"request[_ -]?id[^0-9a-fA-F]*(?:0x)?([0-9a-fA-F]{64})",text,re.I)}
if len(matches)!=1: raise SystemExit(1)
print(matches.pop())
' "$TMP/response.stdout" "$TMP/response.stderr")" || {
  python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$STATUS" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys,tempfile
target,canister,root,caller,status,response_sha,completed,preflight,stdout_path,stderr_path,*argv=sys.argv[1:]
value={'schema_version':3,'stage':'controller_update_uncertain','observed_at_unix':int(completed),
       'bridge_canister_id':canister,'sns_root_canister_id':root,'executing_principal':caller,
       'command_argv':argv,'request_id':'','response_exit_code':int(status),
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,**json.load(open(preflight))}
parent=os.path.dirname(os.path.abspath(target)) or '.'
fd,tmp=tempfile.mkstemp(prefix='.handover-evidence.',dir=parent)
out=os.fdopen(fd,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
  echo "INCIDENT: controller handover succeeded but the ICP CLI omitted the request ID; durable uncertain checkpoint retained" >&2
  exit 1
}
# Persist the irreversible request before attempting the public postcondition.
python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$REQUEST_ID" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys,tempfile
target,canister,root,caller,request_id,response_sha,completed,preflight,stdout_path,stderr_path,*argv=sys.argv[1:]
value={'schema_version':3,'stage':'controller_update_submitted','observed_at_unix':int(completed),
       'bridge_canister_id':canister,'sns_root_canister_id':root,'executing_principal':caller,
       'command_argv':argv,'request_id':request_id,'response_exit_code':0,
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,**json.load(open(preflight))}
parent=os.path.dirname(os.path.abspath(target)) or '.'
fd,tmp=tempfile.mkstemp(prefix='.handover-evidence.',dir=parent)
out=os.fdopen(fd,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
if ! icp canister status bridge-canister -e production --public --json >"$TMP/postcondition-status.json"; then
  echo "INCIDENT: controller handover succeeded but the public postcondition could not be read; submitted checkpoint retained" >&2
  exit 1
fi
for method in get_bridge_status get_production_lifecycle get_runtime_binding storage_integrity_check get_activation_status get_activation_attestation; do
  if ! icp canister call bridge-canister "$method" '()' -e production --json >"$TMP/post-$method.json"; then
    echo "INCIDENT: controller handover succeeded but the post-handover state snapshot is incomplete; submitted checkpoint retained" >&2
    exit 1
  fi
done
python3 - "$TMP/preflight.json" "$TMP/postcondition-status.json" "$ROOT" "$EXPECTED_WASM" "$TMP/post-get_bridge_status.json" "$TMP/post-get_production_lifecycle.json" "$TMP/post-get_runtime_binding.json" "$TMP/post-storage_integrity_check.json" "$TMP/post-get_activation_status.json" "$TMP/post-get_activation_attestation.json" >"$TMP/postcondition.json" <<'PY'
import hashlib,json,sys
preflight_path,status_path,root,expected_wasm,bridge_path,lifecycle_path,runtime_path,integrity_path,activation_path,attestation_path=sys.argv[1:]
preflight=json.load(open(preflight_path)); value=json.load(open(status_path))
def values(item,key):
 out=[]
 if isinstance(item,dict):
  for k,v in item.items():
   if k==key: out.append(v)
   out.extend(values(v,key))
 elif isinstance(item,list):
  for v in item: out.extend(values(v,key))
 return out
found=values(value,'controllers')
if len(found)!=1 or not isinstance(found[0],list):
 raise SystemExit('public canister status does not contain one controller list')
controllers=[str(controller) for controller in found[0]]
if controllers != [root]:
 raise SystemExit('INCIDENT: live controller postcondition is not KINIC SNS Root-only')
modules=values(value,'module_hash') or values(value,'module')
if len(modules)!=1: raise SystemExit('public canister status does not contain one module hash')
module=str(modules[0]).strip().strip('"').lower().removeprefix('0x')
if module != expected_wasm.lower().removeprefix('0x'): raise SystemExit('INCIDENT: module changed during controller handover')
def load(path): return json.load(open(path))
bridge,lifecycle,runtime,integrity,activation,attestation=map(load,[bridge_path,lifecycle_path,runtime_path,integrity_path,activation_path,attestation_path])
def scalar(item,key):
 found=values(item,key)
 if len(found)!=1: raise SystemExit(f'continuity snapshot lacks one {key}')
 return found[0]
if scalar(bridge,'deposits_paused') is not False or scalar(bridge,'sufficient') is not True:
 raise SystemExit('INCIDENT: IC deposit admission or reserve changed during handover')
if lifecycle != {'Ok':{'Activated':None}}:
 raise SystemExit('INCIDENT: production lifecycle is not Activated after handover')
if scalar(integrity,'Ok') != 'ok':
 raise SystemExit('INCIDENT: storage integrity is not ok after handover')
if scalar(attestation,'deposits_paused') is not False or scalar(attestation,'withdrawals_paused') is not False:
 raise SystemExit('INCIDENT: Base admission is paused after handover')
if scalar(activation,'deposits_paused') is not False:
 raise SystemExit('INCIDENT: activation status is paused after handover')
def prior(prefix):
 raw=bytes.fromhex(preflight[prefix+'_response_json_hex'])
 if hashlib.sha256(raw).hexdigest()!=preflight[prefix+'_response_sha256']: raise SystemExit(f'pre-handover {prefix} digest mismatch')
 return json.loads(raw)
if prior('before_runtime_binding') != runtime: raise SystemExit('INCIDENT: RuntimeBinding changed during handover')
if prior('before_lifecycle') != lifecycle: raise SystemExit('INCIDENT: lifecycle changed during handover')
if prior('before_activation_status') != activation: raise SystemExit('INCIDENT: activation completion changed during handover')
before_bridge=prior('before_bridge_status')
for key in ('mint_authorization_ttl_seconds','mint_authorization_epoch'):
 if scalar(before_bridge,key)!=scalar(bridge,key): raise SystemExit(f'INCIDENT: {key} changed during handover')
for key in ('deposits','withdrawals'):
 if int(scalar(bridge,key)) < int(scalar(before_bridge,key)): raise SystemExit(f'INCIDENT: {key} record count regressed during handover')
before_audit=int(scalar(before_bridge,'retained_audit_events'))+int(scalar(before_bridge,'pruned_audit_events'))
after_audit=int(scalar(bridge,'retained_audit_events'))+int(scalar(bridge,'pruned_audit_events'))
if after_audit < before_audit: raise SystemExit('INCIDENT: audit sequence regressed during handover')
def evidence(path,prefix):
 raw=open(path,'rb').read()
 return {prefix+'_response_json_hex':raw.hex(),prefix+'_response_sha256':hashlib.sha256(raw).hexdigest()}
snapshot={'final_controllers':controllers,'after_module_sha256':module}
for path,prefix in [(status_path,'after_management_status'),(bridge_path,'after_bridge_status'),
                    (lifecycle_path,'after_lifecycle'),(runtime_path,'after_runtime_binding'),
                    (integrity_path,'after_storage_integrity'),(activation_path,'after_activation_status'),
                    (attestation_path,'after_activation_attestation')]: snapshot.update(evidence(path,prefix))
print(json.dumps(snapshot,sort_keys=True,separators=(',',':')))
PY
COMPLETED_AT="$(date +%s)"
python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$REQUEST_ID" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/postcondition.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys,tempfile
target,canister,root,caller,request_id,response_sha,completed,preflight,postcondition,stdout_path,stderr_path,*argv=sys.argv[1:]
metrics=json.load(open(preflight))
post=json.load(open(postcondition)); final_controllers=post['final_controllers']
value={'schema_version':3,'stage':'complete','observed_at_unix':int(completed),'bridge_canister_id':canister,
       'sns_root_canister_id':root,'executing_principal':caller,'command_argv':argv,
       'request_id':request_id,'response_exit_code':0,
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),
       'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,'final_controllers':final_controllers,**metrics,**post}
parent=os.path.dirname(os.path.abspath(target)) or '.'
fd,tmp=tempfile.mkstemp(prefix='.handover-evidence.',dir=parent)
out=os.fdopen(fd,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
if ! "$BRIDGE_HANDOVER_VALIDATOR_BIN" validate-controller-handover-completion \
  "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT" \
  "$BRIDGE_CONTROLLER_SCHEDULE_RECEIPT" "$BRIDGE_CONTROLLER_ACTIVATION_RECEIPT" \
  "$BRIDGE_HANDOVER_EVIDENCE_FILE"; then
  echo "INCIDENT: controller handover completed but durable completion evidence failed typed validation" >&2
  exit 1
fi
echo "controller handover completed; evidence=$BRIDGE_HANDOVER_EVIDENCE_FILE" >&2
