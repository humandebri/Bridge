#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-handover-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/source/scripts" "$T/source/src" "$T/bin" "$T/bundle"
cp "$ROOT/scripts/production-handover-driver.sh" "$ROOT/scripts/production-validation.sh" "$T/source/scripts/"
cat >"$T/source/scripts/ci-local.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == proofs ]]
printf 'proofs %s\n' "$*" >>"$TRACE"
if [[ "${MUTATE_HANDOVER_INPUT_AFTER_FREEZE:-false}" == true ]]; then
  printf '{"bridge_canister_id":"rrkah-fqaaa-aaaaa-aaaaq-cai"}\n' >"$ORIGINAL_PROFILE"
fi
if [[ "${MUTATE_HANDOVER_RECEIPT_AFTER_FREEZE:-false}" == true ]]; then
  printf '{"kind":"execute","schedule_receipt_sha256":"wrong"}\n' >"$ORIGINAL_EXECUTE_RECEIPT"
fi
if [[ "${PROOF_GATE_FAIL:-false}" == true ]]; then exit 42; fi
SH
cat >"$T/source/scripts/rebuild-release-artifacts.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'rebuild %s\n' "$*" >>"$TRACE"
[[ "${REPRODUCIBLE_BUILD_FAIL:-false}" != true ]]
SH
chmod +x "$T/source/scripts/ci-local.sh"
chmod +x "$T/source/scripts/rebuild-release-artifacts.sh"
printf '/target\n' >"$T/source/.gitignore"
cat >"$T/source/Cargo.toml" <<'TOML'
[package]
name = "bridge-profile"
version = "0.0.0"
edition = "2021"
TOML
cat >"$T/source/Cargo.lock" <<'LOCK'
version = 4
[[package]]
name = "bridge-profile"
version = "0.0.0"
LOCK
cat >"$T/source/src/main.rs" <<'RS'
use std::{env,fs};
fn main(){let a:Vec<String>=env::args().skip(1).collect();if a[0]=="verify-production-canister-handover"{let counter=env::var("TRACE").unwrap()+".verify";let n=fs::read_to_string(&counter).ok().and_then(|v|v.parse::<u32>().ok()).unwrap_or(0);fs::write(&counter,(n+1).to_string()).unwrap();let valid=a.len()==5&&fs::read_to_string(&a[2]).is_ok_and(|v|v.contains("\"kind\":\"seal\"")&&v.contains("\"initial_operational_parameters_sha256\":\"1111\""))&&fs::read_to_string(&a[3]).is_ok_and(|v|v.contains("\"kind\":\"schedule\"")&&v.contains("\"seal_receipt_sha256\":\"2222\""))&&fs::read_to_string(&a[4]).is_ok_and(|v|v.contains("\"kind\":\"execute\"")&&v.contains("\"schedule_receipt_sha256\":\"3333\""));if (n>0&&env::var("HANDOVER_PRE_SEND_ACTIVE_DRIFT").as_deref()==Ok("true"))||!valid||["HANDOVER_BOOTSTRAP","HANDOVER_SEALED","HANDOVER_ATTESTATION_MISSING","HANDOVER_ATTESTATION_STALE","HANDOVER_ATTESTATION_PREDEPLOY","HANDOVER_PROFILE_DRIFT","HANDOVER_CONTROLLER_DRIFT","HANDOVER_MODULE_DRIFT","HANDOVER_INITIAL_PARAMETERS_DRIFT","HANDOVER_SEAL_RECEIPT_DRIFT","HANDOVER_SCHEDULE_RECEIPT_DRIFT","HANDOVER_EXECUTE_RECEIPT_DRIFT","HANDOVER_RUNTIME_BINDING_DRIFT","HANDOVER_RESERVE_DRIFT","HANDOVER_STORAGE_INTEGRITY_DRIFT","HANDOVER_IC_DEPOSITS_PAUSED","HANDOVER_BASE_DEPOSITS_PAUSED","HANDOVER_BASE_WITHDRAWALS_PAUSED"].iter().any(|name|env::var(name).as_deref()==Ok("true")){std::process::exit(1)}println!("production_canister_handover=verified")}else if a[0]=="verify-production-canister-predeploy"{println!("production_canister_predeploy=verified")}else if a[0]=="validate-production-handover-candidate"{println!("production_handover_candidate=pass manifest_sha256={}","a".repeat(64))}else if a[0]=="validate-bundle"&&env::var("REJECT_CURRENT_GATE_B").as_deref()==Ok("true"){std::process::exit(1)}else{println!("gate_b=pre_seal-pass authorizing=seal manifest_sha256={}","a".repeat(64))}}
RS
git -C "$T/source" init -q
git -C "$T/source" config user.email bridge-test@example.invalid
git -C "$T/source" config user.name bridge-test
git -C "$T/source" add .
git -C "$T/source" commit -qm 'handover fixture'
REVISION="$(git -C "$T/source" rev-parse HEAD)"
TREE="$(git -C "$T/source" archive HEAD | shasum -a 256 | awk '{print $1}')"
printf '{"kind":"production-controller-bootstrap-upgrade","source_revision":"%s","source_tree_sha256":"%s"}\n' \
  "$REVISION" "$TREE" >"$T/bundle/production-canister-upgrade-receipt.json"
cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"2vxsx-fae","root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","bridge_canister_wasm_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","parameters":{"cycles_floor":"1000"}}
JSON
PROFILE_SHA="$(shasum -a 256 "$T/bundle/profile.json" | awk '{print $1}')"
printf '{"schema_version":3,"from_source_revision":"%s","from_source_tree_sha256":"%s","upgrade_source_revision":"%s","upgrade_source_tree_sha256":"%s","to_source_revision":"%s","to_source_tree_sha256":"%s"}\n' \
  "$REVISION" "$TREE" "$REVISION" "$TREE" "$REVISION" "$TREE" \
  >"$T/bundle/post-gate-a-policy-transition.json"
TRANSITION_SHA="$(shasum -a 256 "$T/bundle/post-gate-a-policy-transition.json" | awk '{print $1}')"
UPGRADE_SHA="$(shasum -a 256 "$T/bundle/production-canister-upgrade-receipt.json" | awk '{print $1}')"
printf '{"source_revision":"%s","source_tree_sha256":"%s","artifacts":[{"path":"profile.json","sha256":"%s"},{"path":"post-gate-a-policy-transition.json","sha256":"%s"},{"path":"production-canister-upgrade-receipt.json","sha256":"%s"}]}\n' \
  "$REVISION" "$TREE" "$PROFILE_SHA" "$TRANSITION_SHA" "$UPGRADE_SHA" >"$T/bundle/release-manifest.json"
GATE_B_HASH="$(printf 'a%.0s' {1..64})"
RAW_MANIFEST_SHA="$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')"
[[ "$RAW_MANIFEST_SHA" != "$GATE_B_HASH" ]]
export TRACE="$T/trace"
export ORIGINAL_PROFILE="$T/bundle/profile.json"
export ORIGINAL_EXECUTE_RECEIPT="$T/controller-execute-receipt.json"
cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
echo "icp $*" >>"$TRACE"
if [[ "$*" == *'identity principal'* ]]; then echo 'aaaaa-aa'
elif [[ "$*" == *'status bridge-canister -e production -i'* ]]; then echo "${HANDOVER_CANISTER_ID:-2vxsx-fae}"
elif [[ "$*" == *get_bridge_status* ]]; then
  calls="$(cat "$TRACE.bridge-calls" 2>/dev/null || printf 0)"; printf '%s\n' "$((calls+1))" >"$TRACE.bridge-calls"; reserve="${HANDOVER_RESERVE_SUFFICIENT:-true}"; paused="${HANDOVER_PAUSED:-false}"
  [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_RESERVE_DRIFT:-false}" != true ]] || reserve=false
  [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_IC_PAUSE_DRIFT:-false}" != true ]] || paused=true
  printf '{"reserve":{"sufficient":%s},"deposits_paused":%s,"mint_authorization_ttl_seconds":900,"mint_authorization_epoch":7,"counts":{"deposits":2,"withdrawals":3,"retained_audit_events":8,"pruned_audit_events":5}}\n' "$reserve" "$paused"
elif [[ "$*" == *get_production_lifecycle* ]]; then
  calls="$(cat "$TRACE.lifecycle-calls" 2>/dev/null || printf 0)"; printf '%s\n' "$((calls+1))" >"$TRACE.lifecycle-calls"; lifecycle="${HANDOVER_LIFECYCLE:-Activated}"
  [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_LIFECYCLE_DRIFT:-false}" != true ]] || lifecycle=Bootstrap
  printf '{"Ok":{"%s":null}}\n' "$lifecycle"
elif [[ "$*" == *get_runtime_binding* ]]; then
  runtime="stable"; [[ -e "$TRACE.updated" ]] && runtime="${HANDOVER_POST_RUNTIME:-stable}"
  calls="$(cat "$TRACE.runtime-calls" 2>/dev/null || printf 0)"; printf '%s\n' "$((calls+1))" >"$TRACE.runtime-calls"; [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_RUNTIME_DRIFT:-false}" != true ]] || runtime=drifted
  printf '{"schema_version":5,"binding":"%s"}\n' "$runtime"
elif [[ "$*" == *storage_integrity_check* ]]; then
  integrity="${HANDOVER_STORAGE_RESULT:-ok}"; [[ -e "$TRACE.updated" ]] && integrity="${HANDOVER_POST_STORAGE_RESULT:-$integrity}"
  calls="$(cat "$TRACE.integrity-calls" 2>/dev/null || printf 0)"; printf '%s\n' "$((calls+1))" >"$TRACE.integrity-calls"; [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_STORAGE_DRIFT:-false}" != true ]] || integrity=corrupt
  printf '{"Ok":"%s"}\n' "$integrity"
elif [[ "$*" == *get_activation_status* ]]; then
  calls="$(cat "$TRACE.activation-calls" 2>/dev/null || printf 0)"; printf '%s\n' "$((calls+1))" >"$TRACE.activation-calls"; paused=false
  [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_ACTIVATION_DRIFT:-false}" != true ]] || paused=true
  printf '{"Ok":{"deposits_paused":%s,"pending_timelock_operation":[],"last_confirmed_activation":[{"phase":"execute"}]}}\n' "$paused"
elif [[ "$*" == *get_activation_attestation* ]]; then
  calls="$(cat "$TRACE.attestation-calls" 2>/dev/null || printf 0)"; printf '%s\n' "$((calls+1))" >"$TRACE.attestation-calls"; deposits="${HANDOVER_BASE_DEPOSITS_PAUSED:-false}"; withdrawals="${HANDOVER_BASE_WITHDRAWALS_PAUSED:-false}"
  [[ "$calls" -lt 1 || "${HANDOVER_PRE_SEND_BASE_PAUSE_DRIFT:-false}" != true ]] || deposits=true
  printf '{"Ok":{"deposits_paused":%s,"withdrawals_paused":%s}}\n' "$deposits" "$withdrawals"
elif [[ "$*" == *'status bridge-canister -e production --identity'* ]]; then
  controller="${HANDOVER_CONTROLLER:-aaaaa-aa}"
  calls="$(grep -c 'status bridge-canister -e production --identity' "$TRACE")"
  if [[ "$calls" -ge 2 && -n "${HANDOVER_SECOND_CONTROLLER:-}" ]]; then controller="$HANDOVER_SECOND_CONTROLLER"; fi
  printf '{"controllers":["%s"],"module_hash":"%s","cycles":%s,"freezing_threshold":86400,"idle_cycles_burned_per_day":100}\n' "$controller" "${HANDOVER_MODULE:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}" "${HANDOVER_CYCLES:-1000000}"
elif [[ "$*" == *'status bridge-canister -e production --public --json'* ]]; then
  [[ "${HANDOVER_POSTCONDITION_FAIL:-false}" != true ]] || exit 1
  printf '{"controllers":%s,"module_hash":"%s"}\n' "${HANDOVER_FINAL_CONTROLLERS:-[\"7jkta-eyaaa-aaaaq-aaarq-cai\"]}" "${HANDOVER_POST_MODULE:-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}"
elif [[ "$*" == *'settings update bridge-canister'* ]]; then
  [[ "${HANDOVER_FAIL:-false}" != true ]] || exit 1
  : >"$TRACE.updated"
  [[ "${HANDOVER_NO_REQUEST_ID:-false}" == true ]] || echo 'request_id=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' >&2
else echo "unexpected ICP CLI call: $*" >&2; exit 1
fi
SH
chmod +x "$T/bin/icp"
export PATH="$T/bin:$PATH"
printf '{"kind":"seal","initial_operational_parameters_sha256":"1111"}\n' >"$T/operational-config-seal-receipt.json"
printf '{"kind":"schedule","seal_receipt_sha256":"2222"}\n' >"$T/controller-schedule-receipt.json"
printf '{"kind":"execute","schedule_receipt_sha256":"3333"}\n' >"$T/controller-execute-receipt.json"

run_handover() {
  local evidence="$1"
  shift
  rm -f "$TRACE.updated"
  rm -f "$TRACE.verify"
  rm -f "$TRACE.bridge-calls" "$TRACE.lifecycle-calls" "$TRACE.runtime-calls" \
    "$TRACE.integrity-calls" "$TRACE.activation-calls" "$TRACE.attestation-calls"
  BRIDGE_GATE_B_MANIFEST_SHA256="$GATE_B_HASH" \
  BRIDGE_RELEASE_BUNDLE="$T/bundle" \
  BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT="$T/operational-config-seal-receipt.json" \
  BRIDGE_CONTROLLER_SCHEDULE_RECEIPT="$T/controller-schedule-receipt.json" \
  BRIDGE_CONTROLLER_ACTIVATION_RECEIPT="$T/controller-execute-receipt.json" \
  BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_EVIDENCE_FILE="$evidence" \
  BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY \
  "$T/source/scripts/production-handover-driver.sh" "$@"
}

run_handover "$T/handover.json"
REJECT_CURRENT_GATE_B=true run_handover "$T/historical-lineage.json"
python3 - "$T/handover.json" "$GATE_B_HASH" <<'PY'
import hashlib,json,sys
v=json.load(open(sys.argv[1]))
assert v['final_controllers']==['7jkta-eyaaa-aaaaq-aaarq-cai']
assert v['schema_version']==3 and v['stage']=='complete'
assert v['source_revision'] and len(v['source_tree_sha256'])==64
assert v['gate_b_manifest_sha256']==sys.argv[2]
assert v['operational_config_seal_receipt_sha256']==hashlib.sha256(open(sys.argv[1].replace('handover.json','operational-config-seal-receipt.json'),'rb').read()).hexdigest()
assert v['controller_schedule_receipt_sha256']==hashlib.sha256(open(sys.argv[1].replace('handover.json','controller-schedule-receipt.json'),'rb').read()).hexdigest()
assert v['controller_execute_receipt_sha256']==hashlib.sha256(open(sys.argv[1].replace('handover.json','controller-execute-receipt.json'),'rb').read()).hexdigest()
assert not any('fee_cycles' in key for key in v)
assert v['cycles_balance']==1000000 and v['required_freezing_cycles']==100
assert v['pre_send_cycles_balance']==1000000 and v['pre_send_required_freezing_cycles']==100
for prefix in ('management_status','bridge_status','lifecycle','runtime_binding','storage_integrity','activation_status','activation_attestation'):
 assert len(v['pre_send_'+prefix+'_response_sha256'])==64
transcript=bytes.fromhex(v['response_stdout_hex'])+bytes.fromhex(v['response_stderr_hex'])
assert v['response_exit_code']==0 and hashlib.sha256(transcript).hexdigest()==v['response_sha256']
assert v['request_id'].encode() in transcript
a=v['command_argv']; assert a.count('--remove-all-controllers')==1 and a.count('--add-controller')==1
assert a[a.index('--add-controller')+1]=='7jkta-eyaaa-aaaaq-aaarq-cai'
PY
rg -q 'settings update bridge-canister -e production --remove-all-controllers --add-controller 7jkta-eyaaa-aaaaq-aaarq-cai --force --identity production --debug' "$TRACE"
rg -q '^proofs proofs$' "$TRACE"
rg -q '^rebuild ' "$TRACE"

cp "$T/bundle/profile.json" "$T/profile.before-race.json"
MUTATE_HANDOVER_INPUT_AFTER_FREEZE=true run_handover "$T/frozen-input-race.json"
mv "$T/profile.before-race.json" "$T/bundle/profile.json"
python3 - "$T/frozen-input-race.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1]))
assert v['bridge_canister_id']=='2vxsx-fae' and v['before_module_sha256']=='a'*64
PY
cp "$T/controller-execute-receipt.json" "$T/execute.before-race.json"
MUTATE_HANDOVER_RECEIPT_AFTER_FREEZE=true run_handover "$T/frozen-receipt-race.json"
mv "$T/execute.before-race.json" "$T/controller-execute-receipt.json"

if BRIDGE_GATE_B_MANIFEST_SHA256="$GATE_B_HASH" \
  BRIDGE_RELEASE_BUNDLE="$T/bundle" \
  BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_EVIDENCE_FILE="$T/predeploy-handover.json" \
  BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY \
  "$T/source/scripts/production-handover-driver.sh" >/dev/null 2>&1; then
  echo "handover accepted no activation lineage receipts" >&2; exit 1
fi
[[ ! -e "$T/predeploy-handover.json" ]]
for receipt in operational-config-seal controller-schedule controller-execute; do
  mv "$T/$receipt-receipt.json" "$T/$receipt-receipt.missing"
  if run_handover "$T/missing-$receipt.json" >/dev/null 2>&1; then
    echo "handover accepted a missing $receipt receipt" >&2; exit 1
  fi
  [[ ! -e "$T/missing-$receipt.json" ]]
  mv "$T/$receipt-receipt.missing" "$T/$receipt-receipt.json"
done
cp "$T/controller-schedule-receipt.json" "$T/controller-schedule-receipt.valid"
printf '{"kind":"schedule","seal_receipt_sha256":"wrong"}\n' >"$T/controller-schedule-receipt.json"
if run_handover "$T/schedule-lineage-drift.json" >/dev/null 2>&1; then
  echo "handover accepted a schedule receipt outside the seal lineage" >&2; exit 1
fi
[[ ! -e "$T/schedule-lineage-drift.json" ]]
mv "$T/controller-schedule-receipt.valid" "$T/controller-schedule-receipt.json"
cp "$T/controller-execute-receipt.json" "$T/controller-execute-receipt.valid"
printf '{"kind":"execute","schedule_receipt_sha256":"wrong"}\n' >"$T/controller-execute-receipt.json"
if run_handover "$T/execute-lineage-drift.json" >/dev/null 2>&1; then
  echo "handover accepted an execute receipt outside the schedule lineage" >&2; exit 1
fi
[[ ! -e "$T/execute-lineage-drift.json" ]]
mv "$T/controller-execute-receipt.valid" "$T/controller-execute-receipt.json"
for scenario in \
  BOOTSTRAP SEALED ATTESTATION_MISSING ATTESTATION_STALE ATTESTATION_PREDEPLOY \
  PROFILE_DRIFT CONTROLLER_DRIFT MODULE_DRIFT INITIAL_PARAMETERS_DRIFT \
  SEAL_RECEIPT_DRIFT SCHEDULE_RECEIPT_DRIFT EXECUTE_RECEIPT_DRIFT \
  RUNTIME_BINDING_DRIFT RESERVE_DRIFT STORAGE_INTEGRITY_DRIFT \
  IC_DEPOSITS_PAUSED BASE_DEPOSITS_PAUSED BASE_WITHDRAWALS_PAUSED; do
  evidence="$T/$(printf '%s' "$scenario" | tr '[:upper:]_' '[:lower:]-').json"
  export "HANDOVER_${scenario}=true"
  if run_handover "$evidence" >/dev/null 2>&1; then
    echo "handover accepted invalid certified Canister state: $scenario" >&2; exit 1
  fi
  unset "HANDOVER_${scenario}"
  [[ ! -e "$evidence" ]]
done

if PROOF_GATE_FAIL=true run_handover "$T/proof-failed.json" >/dev/null 2>&1; then
  echo "handover accepted a failed proof gate" >&2; exit 1
fi
[[ ! -e "$T/proof-failed.json" ]]
if REPRODUCIBLE_BUILD_FAIL=true run_handover "$T/rebuild-failed.json" >/dev/null 2>&1; then
  echo "handover accepted a failed reproducible artifact build" >&2; exit 1
fi
[[ ! -e "$T/rebuild-failed.json" ]]

updates_before="$(rg -c 'settings update bridge-canister' "$TRACE" || true)"
if HANDOVER_PRE_SEND_ACTIVE_DRIFT=true run_handover "$T/pre-send-active-drift.json" >/dev/null 2>&1; then
  echo "handover accepted active-state drift after evidence reservation" >&2; exit 1
fi
updates_after="$(rg -c 'settings update bridge-canister' "$TRACE" || true)"
[[ "$updates_before" == "$updates_after" ]]
[[ -f "$T/pre-send-active-drift.json" && ! -s "$T/pre-send-active-drift.json" ]]

for scenario in RESERVE_DRIFT IC_PAUSE_DRIFT LIFECYCLE_DRIFT RUNTIME_DRIFT STORAGE_DRIFT ACTIVATION_DRIFT BASE_PAUSE_DRIFT; do
  evidence="$T/pre-send-$(printf '%s' "$scenario" | tr '[:upper:]_' '[:lower:]-').json"
  export "HANDOVER_PRE_SEND_${scenario}=true"
  updates_before="$(rg -c 'settings update bridge-canister' "$TRACE" || true)"
  if run_handover "$evidence" >/dev/null 2>&1; then
    echo "handover accepted pre-send live-state drift: $scenario" >&2; exit 1
  fi
  updates_after="$(rg -c 'settings update bridge-canister' "$TRACE" || true)"
  unset "HANDOVER_PRE_SEND_${scenario}"
  [[ "$updates_before" == "$updates_after" && -f "$evidence" && ! -s "$evidence" ]]
done

if HANDOVER_CANISTER_ID=rrkah-fqaaa-aaaaa-aaaaq-cai run_handover "$T/wrong-canister.json" >/dev/null 2>&1; then
  echo "handover accepted a production mapping drift" >&2; exit 1
fi
[[ ! -e "$T/wrong-canister.json" ]]
if HANDOVER_CONTROLLER=2vxsx-fae run_handover "$T/not-controller.json" >/dev/null 2>&1; then
  echo "handover accepted an identity that is not a current controller" >&2; exit 1
fi
[[ ! -e "$T/not-controller.json" ]]
if HANDOVER_CYCLES=999 run_handover "$T/low-cycles.json" >/dev/null 2>&1; then
  echo "handover accepted a balance below the approved cycles floor" >&2; exit 1
fi
[[ ! -e "$T/low-cycles.json" ]]
updates_before="$(rg -c 'settings update bridge-canister' "$TRACE" || true)"
if HANDOVER_SECOND_CONTROLLER=2vxsx-fae run_handover "$T/pre-send-controller-race.json" >/dev/null 2>&1; then
  echo "handover accepted a second controller introduced immediately before send" >&2; exit 1
fi
updates_after="$(rg -c 'settings update bridge-canister' "$TRACE" || true)"
[[ "$updates_before" == "$updates_after" ]]
[[ -f "$T/pre-send-controller-race.json" && ! -s "$T/pre-send-controller-race.json" ]]
if HANDOVER_FINAL_CONTROLLERS='["7jkta-eyaaa-aaaaq-aaarq-cai","aaaaa-aa"]' run_handover "$T/extra-controller.json" >/dev/null 2>&1; then
  echo "handover accepted an extra live controller" >&2; exit 1
fi
python3 - "$T/extra-controller.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_FINAL_CONTROLLERS='["aaaaa-aa"]' run_handover "$T/missing-root.json" >/dev/null 2>&1; then
  echo "handover accepted a live controller set without SNS Root" >&2; exit 1
fi
python3 - "$T/missing-root.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_POST_MODULE="$(printf 'b%.0s' {1..64})" run_handover "$T/post-module-drift.json" >/dev/null 2>&1; then
  echo "handover accepted a module change across controller handover" >&2; exit 1
fi
python3 - "$T/post-module-drift.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_POST_RUNTIME=drifted run_handover "$T/post-runtime-drift.json" >/dev/null 2>&1; then
  echo "handover accepted RuntimeBinding drift across controller handover" >&2; exit 1
fi
python3 - "$T/post-runtime-drift.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_POST_STORAGE_RESULT=corrupt run_handover "$T/post-storage-drift.json" >/dev/null 2>&1; then
  echo "handover accepted storage integrity drift across controller handover" >&2; exit 1
fi
python3 - "$T/post-storage-drift.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_POSTCONDITION_FAIL=true run_handover "$T/postcondition-failed.json" >/dev/null 2>&1; then
  echo "handover wrote evidence without a live controller postcondition" >&2; exit 1
fi
python3 - "$T/postcondition-failed.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_submitted'
PY
printf '\n' >>"$T/source/src/main.rs"
if run_handover "$T/dirty.json" >/dev/null 2>&1; then
  echo "handover accepted a dirty source tree" >&2; exit 1
fi
[[ ! -e "$T/dirty.json" ]]
git -C "$T/source" restore src/main.rs
if run_handover "$T/argument.json" arbitrary-controller >/dev/null 2>&1; then
  echo "handover accepted an arbitrary driver argument" >&2; exit 1
fi
[[ ! -e "$T/argument.json" ]]
if HANDOVER_FAIL=true run_handover "$T/failed.json" >/dev/null 2>&1; then
  echo "handover accepted a failed controller update" >&2; exit 1
fi
python3 - "$T/failed.json" <<'PY'
import hashlib,json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==3 and v['stage']=='controller_update_uncertain'
assert v['response_exit_code']!=0
transcript=bytes.fromhex(v['response_stdout_hex'])+bytes.fromhex(v['response_stderr_hex'])
assert hashlib.sha256(transcript).hexdigest()==v['response_sha256']
PY
if HANDOVER_NO_REQUEST_ID=true run_handover "$T/missing-request-id.json" >/dev/null 2>&1; then
  echo "handover accepted a success response without a request ID" >&2; exit 1
fi
python3 - "$T/missing-request-id.json" <<'PY'
import hashlib,json,sys
v=json.load(open(sys.argv[1])); assert v['stage']=='controller_update_uncertain' and v['response_exit_code']==0 and v['request_id']==''
transcript=bytes.fromhex(v['response_stdout_hex'])+bytes.fromhex(v['response_stderr_hex'])
assert hashlib.sha256(transcript).hexdigest()==v['response_sha256']
PY
