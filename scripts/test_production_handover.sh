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
if [[ "${PROOF_GATE_FAIL:-false}" == true ]]; then exit 42; fi
SH
cat >"$T/source/scripts/rebuild-release-artifacts.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'rebuild %s\n' "$*" >>"$TRACE"
[[ "${REPRODUCIBLE_BUILD_FAIL:-false}" != true ]]
SH
cat >"$T/source/scripts/production-live-preflight.sh" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'live-preflight %s\n' "$*" >>"$TRACE"
[[ "$#" == 3 && "$1" == verify-handover-deployment && -f "$2" && -f "$3" ]]
: "${BRIDGE_MONITOR_RPC_URL_1:?}" "${BRIDGE_MONITOR_RPC_URL_2:?}" "${BRIDGE_MONITOR_RPC_URL_3:?}"
[[ "${HANDOVER_LIVE_BASE_DRIFT:-false}" != true ]]
SH
chmod +x "$T/source/scripts/ci-local.sh"
chmod +x "$T/source/scripts/rebuild-release-artifacts.sh"
chmod +x "$T/source/scripts/production-live-preflight.sh"
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
fn main(){let a:Vec<String>=env::args().skip(1).collect();if a[0]=="validate-production-handover-receipt"{if a.len()!=5||env::var("HANDOVER_GATE_A_RECEIPT_DRIFT").as_deref()==Ok("true")||env::var("HANDOVER_DEPLOYMENT_BINDING_DRIFT").as_deref()==Ok("true")||!fs::read_to_string(&a[2]).unwrap().contains("\"schema_version\":2"){std::process::exit(1)}println!("{}","c".repeat(64))}else if a[0]=="verify-production-canister-predeploy"{println!("production_canister_predeploy=verified")}else{println!("gate_a=pass authorizing=true manifest_sha256={}","a".repeat(64))}}
RS
git -C "$T/source" init -q
git -C "$T/source" config user.email bridge-test@example.invalid
git -C "$T/source" config user.name bridge-test
git -C "$T/source" add .
git -C "$T/source" commit -qm 'handover fixture'
REVISION="$(git -C "$T/source" rev-parse HEAD)"
TREE="$(git -C "$T/source" archive HEAD | shasum -a 256 | awk '{print $1}')"
printf '{"source_revision":"%s","source_tree_sha256":"%s"}\n' "$REVISION" "$TREE" >"$T/bundle/release-manifest.json"
cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"2vxsx-fae","root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","parameters":{"cycles_floor":"1000"}}
JSON
export TRACE="$T/trace"
cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
echo "icp $*" >>"$TRACE"
if [[ "$*" == *'identity principal'* ]]; then echo 'aaaaa-aa'
elif [[ "$*" == *'status bridge-canister -e production -i'* ]]; then echo "${HANDOVER_CANISTER_ID:-2vxsx-fae}"
elif [[ "$*" == *get_bridge_status* ]]; then printf '{"deposits_paused":%s}\n' "${HANDOVER_PAUSED:-true}"
elif [[ "$*" == *'status bridge-canister -e production --identity'* ]]; then printf '{"controllers":["%s"],"cycles":%s,"freezing_threshold":86400,"idle_cycles_burned_per_day":100}\n' "${HANDOVER_CONTROLLER:-aaaaa-aa}" "${HANDOVER_CYCLES:-1000000}"
elif [[ "$*" == *'status bridge-canister -e production --public --json'* ]]; then
  [[ "${HANDOVER_POSTCONDITION_FAIL:-false}" != true ]] || exit 1
  printf '{"controllers":%s}\n' "${HANDOVER_FINAL_CONTROLLERS:-[\"7jkta-eyaaa-aaaaq-aaarq-cai\"]}"
elif [[ "$*" == *'settings update bridge-canister'* ]]; then
  [[ "${HANDOVER_FAIL:-false}" != true ]] || exit 1
  [[ "${HANDOVER_NO_REQUEST_ID:-false}" == true ]] || echo 'request_id=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' >&2
else echo "unexpected ICP CLI call: $*" >&2; exit 1
fi
SH
chmod +x "$T/bin/icp"
export PATH="$T/bin:$PATH"
printf '{}\n' >"$T/production-canister-install-receipt.json"
printf '{"schema_version":2}\n' >"$T/gate-a-receipt.json"
printf '{}\n' >"$T/deployment-binding.json"

run_handover() {
  local evidence="$1"
  shift
  BRIDGE_GATE_A_MANIFEST_SHA256="$(printf 'a%.0s' {1..64})" \
  BRIDGE_RELEASE_BUNDLE="$T/bundle" \
  BRIDGE_CANISTER_INSTALL_RECEIPT="$T/production-canister-install-receipt.json" \
  BRIDGE_GATE_A_RECEIPT="$T/gate-a-receipt.json" \
  BRIDGE_DEPLOYMENT_BINDING_FILE="$T/deployment-binding.json" \
  BRIDGE_MONITOR_RPC_URL_1=https://one.example \
  BRIDGE_MONITOR_RPC_URL_2=https://two.example \
  BRIDGE_MONITOR_RPC_URL_3=https://three.example \
  BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_EVIDENCE_FILE="$evidence" \
  BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY \
  "$T/source/scripts/production-handover-driver.sh" "$@"
}

run_handover "$T/handover.json"
python3 - "$T/handover.json" <<'PY'
import hashlib,json,sys
v=json.load(open(sys.argv[1]))
assert v['final_controllers']==['7jkta-eyaaa-aaaaq-aaarq-cai']
assert v['schema_version']==2 and v['stage']=='complete'
assert v['cycles_balance']==1000000 and v['required_freezing_cycles']==100
transcript=bytes.fromhex(v['response_stdout_hex'])+bytes.fromhex(v['response_stderr_hex'])
assert v['response_exit_code']==0 and hashlib.sha256(transcript).hexdigest()==v['response_sha256']
assert v['request_id'].encode() in transcript
a=v['command_argv']; assert a.count('--remove-all-controllers')==1 and a.count('--add-controller')==1
assert a[a.index('--add-controller')+1]=='7jkta-eyaaa-aaaaq-aaarq-cai'
PY
rg -q 'settings update bridge-canister -e production --remove-all-controllers --add-controller 7jkta-eyaaa-aaaaq-aaarq-cai --force --identity production --debug' "$TRACE"
rg -q '^proofs proofs$' "$TRACE"
rg -q '^rebuild ' "$TRACE"

if BRIDGE_GATE_A_MANIFEST_SHA256="$(printf 'a%.0s' {1..64})" \
  BRIDGE_RELEASE_BUNDLE="$T/bundle" \
  BRIDGE_CANISTER_INSTALL_RECEIPT="$T/production-canister-install-receipt.json" \
  BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_EVIDENCE_FILE="$T/predeploy-handover.json" \
  BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY \
  "$T/source/scripts/production-handover-driver.sh" >/dev/null 2>&1; then
  echo "handover accepted install evidence without a completed Gate A receipt" >&2; exit 1
fi
[[ ! -e "$T/predeploy-handover.json" ]]
if BRIDGE_GATE_A_MANIFEST_SHA256="$(printf 'a%.0s' {1..64})" \
  BRIDGE_RELEASE_BUNDLE="$T/bundle" \
  BRIDGE_CANISTER_INSTALL_RECEIPT="$T/production-canister-install-receipt.json" \
  BRIDGE_GATE_A_RECEIPT="$T/gate-a-receipt.json" \
  BRIDGE_MONITOR_RPC_URL_1=https://one.example \
  BRIDGE_MONITOR_RPC_URL_2=https://two.example \
  BRIDGE_MONITOR_RPC_URL_3=https://three.example \
  BRIDGE_ICP_IDENTITY=production \
  BRIDGE_HANDOVER_EVIDENCE_FILE="$T/missing-binding.json" \
  BRIDGE_HANDOVER_CONFIRMATION=TRANSFER_TO_KINIC_SNS_ROOT_ONLY \
  "$T/source/scripts/production-handover-driver.sh" >/dev/null 2>&1; then
  echo "handover accepted no canonical deployment binding" >&2; exit 1
fi
[[ ! -e "$T/missing-binding.json" ]]
if HANDOVER_GATE_A_RECEIPT_DRIFT=true run_handover "$T/gate-a-receipt-drift.json" >/dev/null 2>&1; then
  echo "handover accepted a Gate A receipt drift" >&2; exit 1
fi
[[ ! -e "$T/gate-a-receipt-drift.json" ]]
if HANDOVER_DEPLOYMENT_BINDING_DRIFT=true run_handover "$T/deployment-binding-drift.json" >/dev/null 2>&1; then
  echo "handover accepted a deployment binding drift" >&2; exit 1
fi
[[ ! -e "$T/deployment-binding-drift.json" ]]
if HANDOVER_LIVE_BASE_DRIFT=true run_handover "$T/live-base-drift.json" >/dev/null 2>&1; then
  echo "handover accepted a live Base deployment drift" >&2; exit 1
fi
[[ ! -e "$T/live-base-drift.json" ]]

if PROOF_GATE_FAIL=true run_handover "$T/proof-failed.json" >/dev/null 2>&1; then
  echo "handover accepted a failed proof gate" >&2; exit 1
fi
[[ ! -e "$T/proof-failed.json" ]]
if REPRODUCIBLE_BUILD_FAIL=true run_handover "$T/rebuild-failed.json" >/dev/null 2>&1; then
  echo "handover accepted a failed reproducible artifact build" >&2; exit 1
fi
[[ ! -e "$T/rebuild-failed.json" ]]

if HANDOVER_PAUSED=false run_handover "$T/unpaused.json" >/dev/null 2>&1; then
  echo "handover accepted an unpaused Bridge" >&2; exit 1
fi
[[ ! -e "$T/unpaused.json" ]]
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
if HANDOVER_FINAL_CONTROLLERS='["7jkta-eyaaa-aaaaq-aaarq-cai","aaaaa-aa"]' run_handover "$T/extra-controller.json" >/dev/null 2>&1; then
  echo "handover accepted an extra live controller" >&2; exit 1
fi
python3 - "$T/extra-controller.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==2 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_FINAL_CONTROLLERS='["aaaaa-aa"]' run_handover "$T/missing-root.json" >/dev/null 2>&1; then
  echo "handover accepted a live controller set without SNS Root" >&2; exit 1
fi
python3 - "$T/missing-root.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==2 and v['stage']=='controller_update_submitted'
PY
if HANDOVER_POSTCONDITION_FAIL=true run_handover "$T/postcondition-failed.json" >/dev/null 2>&1; then
  echo "handover wrote evidence without a live controller postcondition" >&2; exit 1
fi
python3 - "$T/postcondition-failed.json" <<'PY'
import json,sys
v=json.load(open(sys.argv[1])); assert v['schema_version']==2 and v['stage']=='controller_update_submitted'
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
v=json.load(open(sys.argv[1])); assert v['schema_version']==2 and v['stage']=='controller_update_uncertain'
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
