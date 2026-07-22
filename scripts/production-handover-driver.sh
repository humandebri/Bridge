#!/usr/bin/env bash
# Atomically transfer the paused production Bridge Canister to the KINIC SNS Root only.
set -euo pipefail
[[ $# -eq 0 ]] || { echo "production handover driver accepts no arguments" >&2; exit 2; }

SOURCE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=production-validation.sh
source "$SOURCE_ROOT/scripts/production-validation.sh"

: "${BRIDGE_GATE_A_MANIFEST_SHA256:?missing Gate A approval}"
: "${BRIDGE_RELEASE_BUNDLE:?missing release bundle}"
: "${BRIDGE_ICP_IDENTITY:?missing reviewed ICP CLI identity}"
: "${BRIDGE_HANDOVER_EVIDENCE_FILE:?missing handover evidence output path}"
: "${BRIDGE_HANDOVER_CONFIRMATION:?set BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY}"
[[ "$BRIDGE_HANDOVER_CONFIRMATION" == TRANSFER_TO_KINIC_SNS_ROOT_ONLY ]] || {
  echo "controller handover requires the exact confirmation phrase" >&2; exit 1;
}
[[ ! -e "$BRIDGE_HANDOVER_EVIDENCE_FILE" ]] || { echo "handover evidence output already exists" >&2; exit 1; }
for tool in icp python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

production_validate_gate gate-a "$BRIDGE_RELEASE_BUNDLE" "$BRIDGE_GATE_A_MANIFEST_SHA256"
PROFILE="$BRIDGE_RELEASE_BUNDLE/profile.json"
read -r CANISTER ROOT CYCLES_FLOOR < <(python3 -c '
import json,sys
p=json.load(open(sys.argv[1])); print(p["bridge_canister_id"],p["root_canister_id"],p["parameters"]["cycles_floor"])
' "$PROFILE")
[[ "$CANISTER" == rlhjx-iyaaa-aaaaf-qcnyq-cai && "$ROOT" == 7jkta-eyaaa-aaaaq-aaarq-cai ]] || {
  echo "handover profile does not bind the fixed production Bridge and KINIC SNS Root" >&2; exit 1;
}
[[ "$(icp canister status bridge-canister -e production -i --identity "$BRIDGE_ICP_IDENTITY")" == "$CANISTER" ]] || {
  echo "production ICP environment does not map the reviewed Bridge Canister" >&2; exit 1;
}

TMP="$(mktemp -d "${TMPDIR:-/tmp}/bridge-handover.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
icp canister call bridge-canister get_bridge_status '()' -e production --json >"$TMP/bridge-status.json"
icp canister status bridge-canister -e production --identity "$BRIDGE_ICP_IDENTITY" --json >"$TMP/canister-status.json"
EXECUTING_PRINCIPAL="$(icp identity principal --identity "$BRIDGE_ICP_IDENTITY")"
python3 - "$TMP/bridge-status.json" "$TMP/canister-status.json" "$EXECUTING_PRINCIPAL" "$CYCLES_FLOOR" >"$TMP/preflight.json" <<'PY'
import json,re,sys
bridge=json.load(open(sys.argv[1])); status=json.load(open(sys.argv[2])); caller=sys.argv[3]; floor=int(sys.argv[4])
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
 if isinstance(value,int): return value
 text=str(value).strip().strip('"').replace('_','')
 return int(text,16) if text.startswith('0x') else int(re.sub(r'[^0-9]','',text) or '0')
paused=values(bridge,'deposits_paused')
if paused != [True]: raise SystemExit('Bridge Canister must remain paused during handover')
controllers=values(status,'controllers')
controllers=controllers[0] if controllers and isinstance(controllers[0],list) else controllers
if not controllers or caller not in [str(v) for v in controllers]: raise SystemExit('handover identity is not a current controller')
cycles_values=values(status,'cycles') or values(status,'cycles_balance')
threshold_values=values(status,'freezing_threshold') or values(status,'freezing_threshold_seconds')
burn_values=values(status,'idle_cycles_burned_per_day')
if not cycles_values or not threshold_values or not burn_values: raise SystemExit('canister status lacks cycles/freezing inputs')
cycles=number(cycles_values[0]); threshold=number(threshold_values[0]); burn=number(burn_values[0])
freeze_required=(burn*threshold+86399)//86400
if cycles < floor or cycles < freeze_required: raise SystemExit('cycles do not satisfy floor and freezing requirement')
print(json.dumps({'cycles_balance':cycles,'freezing_threshold_seconds':threshold,'idle_cycles_burned_per_day':burn,'required_freezing_cycles':freeze_required},sort_keys=True,separators=(',',':')))
PY

# Prove that the durable evidence target supports create, fsync and atomic
# replacement before changing the controller set.
production_reserve_output "$BRIDGE_HANDOVER_EVIDENCE_FILE" "handover evidence"

COMMAND=(icp canister settings update bridge-canister -e production --remove-all-controllers --add-controller "$ROOT" --force --identity "$BRIDGE_ICP_IDENTITY" --debug)
STARTED_AT="$(date +%s)"
set +e
"${COMMAND[@]}" >"$TMP/response.stdout" 2>"$TMP/response.stderr"
STATUS=$?
set -e
if [[ $STATUS -ne 0 ]]; then
  COMPLETED_AT="$(date +%s)"
  RESPONSE_SHA256="$(python3 -c 'import hashlib,sys; print(hashlib.sha256(open(sys.argv[1],"rb").read()+open(sys.argv[2],"rb").read()).hexdigest())' "$TMP/response.stdout" "$TMP/response.stderr")"
  python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$STATUS" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys
target,canister,root,caller,status,response_sha,completed,preflight,stdout_path,stderr_path,*argv=sys.argv[1:]
value={'schema_version':2,'stage':'controller_update_uncertain','observed_at_unix':int(completed),
       'bridge_canister_id':canister,'sns_root_canister_id':root,'executing_principal':caller,
       'command_argv':argv,'request_id':'','response_exit_code':int(status),
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,**json.load(open(preflight))}
tmp=target+'.tmp'; out=open(tmp,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
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
m=re.search(r"request[_ -]?id[^0-9a-fA-F]*(?:0x)?([0-9a-fA-F]{64})",text,re.I)
if not m: raise SystemExit(1)
print(m.group(1).lower())
' "$TMP/response.stdout" "$TMP/response.stderr")" || {
  python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$STATUS" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys
target,canister,root,caller,status,response_sha,completed,preflight,stdout_path,stderr_path,*argv=sys.argv[1:]
value={'schema_version':2,'stage':'controller_update_uncertain','observed_at_unix':int(completed),
       'bridge_canister_id':canister,'sns_root_canister_id':root,'executing_principal':caller,
       'command_argv':argv,'request_id':'','response_exit_code':int(status),
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,**json.load(open(preflight))}
tmp=target+'.tmp'; out=open(tmp,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
  echo "INCIDENT: controller handover succeeded but the ICP CLI omitted the request ID; durable uncertain checkpoint retained" >&2
  exit 1
}
# Persist the irreversible request before attempting the public postcondition.
python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$REQUEST_ID" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys
target,canister,root,caller,request_id,response_sha,completed,preflight,stdout_path,stderr_path,*argv=sys.argv[1:]
value={'schema_version':2,'stage':'controller_update_submitted','observed_at_unix':int(completed),
       'bridge_canister_id':canister,'sns_root_canister_id':root,'executing_principal':caller,
       'command_argv':argv,'request_id':request_id,'response_exit_code':0,
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,**json.load(open(preflight))}
tmp=target+'.tmp'; out=open(tmp,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
if ! icp canister status bridge-canister -e production --public --json >"$TMP/postcondition-status.json"; then
  echo "INCIDENT: controller handover succeeded but the public postcondition could not be read; submitted checkpoint retained" >&2
  exit 1
fi
python3 - "$TMP/postcondition-status.json" "$ROOT" >"$TMP/postcondition.json" <<'PY'
import json,sys
value=json.load(open(sys.argv[1])); root=sys.argv[2]
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
print(json.dumps({'final_controllers':controllers},sort_keys=True,separators=(',',':')))
PY
COMPLETED_AT="$(date +%s)"
python3 - "$BRIDGE_HANDOVER_EVIDENCE_FILE" "$CANISTER" "$ROOT" "$EXECUTING_PRINCIPAL" "$REQUEST_ID" "$RESPONSE_SHA256" "$COMPLETED_AT" "$TMP/preflight.json" "$TMP/postcondition.json" "$TMP/response.stdout" "$TMP/response.stderr" "${COMMAND[@]}" <<'PY'
import json,os,sys
target,canister,root,caller,request_id,response_sha,completed,preflight,postcondition,stdout_path,stderr_path,*argv=sys.argv[1:]
metrics=json.load(open(preflight))
final_controllers=json.load(open(postcondition))['final_controllers']
value={'schema_version':2,'stage':'complete','observed_at_unix':int(completed),'bridge_canister_id':canister,
       'sns_root_canister_id':root,'executing_principal':caller,'command_argv':argv,
       'request_id':request_id,'response_exit_code':0,
       'response_stdout_hex':open(stdout_path,'rb').read().hex(),
       'response_stderr_hex':open(stderr_path,'rb').read().hex(),
       'response_sha256':response_sha,'final_controllers':final_controllers,**metrics}
tmp=target+'.tmp'; out=open(tmp,'w'); json.dump(value,out,sort_keys=True,separators=(',',':')); out.write('\n'); out.flush(); os.fsync(out.fileno()); out.close(); os.replace(tmp,target)
fd=os.open(os.path.dirname(os.path.abspath(target)) or '.',os.O_RDONLY); os.fsync(fd); os.close(fd)
PY
echo "controller handover completed; evidence=$BRIDGE_HANDOVER_EVIDENCE_FILE" >&2
