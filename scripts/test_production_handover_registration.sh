#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-handover-registration-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/bundle" "$T/source/scripts"
cp "$ROOT/scripts/production-handover-registration-proposal.sh" "$T/source/scripts/"
cat >"$T/source/scripts/production-validation.sh" <<'SH'
production_freeze_bundle(){ cp -R "$1/." "$2/"; }
production_freeze_receipt(){ cp "$1" "$2"; }
production_validate_gate(){ printf 'validate %s\n' "$*" >>"$TRACE"; : >"$BRIDGE_HANDOVER_VALIDATOR_BIN"; [[ "${REGISTRATION_GATE_FAIL:-false}" != true ]]; }
SH
cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"2vxsx-fae","root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","bridge_canister_wasm_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
JSON
cat >"$T/bundle/release-manifest.json" <<'JSON'
{"release_id":"release-1","source_revision":"revision-1","source_tree_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
JSON
MANIFEST_SHA="$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')"
cat >"$T/preparation.json" <<JSON
{"schema_version":5,"stage":"co_controller_ready","source_revision":"revision-1","source_tree_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","gate_b_manifest_sha256":"$MANIFEST_SHA","bridge_canister_id":"2vxsx-fae","sns_root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","final_controllers":["aaaaa-aa","7jkta-eyaaa-aaaaq-aaarq-cai"]}
JSON
cat >"$T/reviewed.json" <<'JSON'
{"schema_version":1,"governance_canister_id":"74ncn-fqaaa-aaaaq-aaasa-cai","root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","bridge_canister_id":"2vxsx-fae","wasm_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","already_registered":false,"registration_proposal":"record { action = opt variant { RegisterDappCanisters = record { canister_ids = vec { principal \"2vxsx-fae\" } } } }"}
JSON
cat >"$T/bin/node" <<'SH'
#!/usr/bin/env bash
if [[ "$*" == *decode-root* ]]; then printf '[]\n'
elif [[ "$*" == *registration-payload* ]]; then printf 'record { action = opt variant { RegisterDappCanisters = record { canister_ids = vec { principal "2vxsx-fae" } } } }\n'
else printf '42\n'
fi
SH
cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$TRACE"
if [[ "$*" == *'identity principal'* ]]; then printf 'aaaaa-aa\n'
elif [[ "$*" == *'status bridge-canister'* ]]; then printf '{"controllers":["7jkta-eyaaa-aaaaq-aaarq-cai","aaaaa-aa"]}\n'
elif [[ "$*" == *list_sns_canisters* ]]; then printf '{"response_bytes":"00"}\n'
elif [[ "$*" == *manage_neuron* ]]; then
  [[ "${REGISTRATION_FAIL:-false}" != true ]] || { printf 'uncertain\n' >&2; exit 1; }
  printf '{"response_bytes":"00"}\n'
else exit 1
fi
SH
chmod +x "$T/bin/node" "$T/bin/icp" "$T/source/scripts/production-handover-registration-proposal.sh"
export PATH="$T/bin:$PATH"
export TRACE="$T/trace"
printf '{}\n' >"$T/seal.json"
printf '{}\n' >"$T/schedule.json"
printf '{}\n' >"$T/execute.json"
export BRIDGE_OPERATIONAL_CONFIG_SEAL_RECEIPT="$T/seal.json"
export BRIDGE_CONTROLLER_SCHEDULE_RECEIPT="$T/schedule.json"
export BRIDGE_CONTROLLER_ACTIVATION_RECEIPT="$T/execute.json"
SUBMIT="$T/source/scripts/production-handover-registration-proposal.sh"

"$SUBMIT" "$T/bundle" "$MANIFEST_SHA" "$T/submission.json" production \
  "$(printf 'c%.0s' {1..64})" aaaaa-aa "$T/preparation.json" "$T/reviewed.json"
python3 - "$T/preparation.json" "$T/reviewed.json" "$T/submission.json" <<'PY'
import hashlib,json,sys
preparation,reviewed,submission=sys.argv[1:]
value=json.load(open(submission))
assert value['schema_version']==1 and value['kind']=='sns-dapp-registration-submission'
assert value['proposal_id']==42 and value['bridge_canister_id']=='2vxsx-fae'
assert value['preparation_receipt_sha256']==hashlib.sha256(open(preparation,'rb').read()).hexdigest()
assert value['reviewed_handover_sha256']==hashlib.sha256(open(reviewed,'rb').read()).hexdigest()
assert value['proposal_command_argv'].count('<fixed-candid-payload>')==1
PY
[[ -s "$T/submission.json.response.json" ]]
[[ "$(rg -c manage_neuron "$TRACE")" == 1 ]]

cp "$T/reviewed.json" "$T/reviewed.valid.json"
python3 - "$T/reviewed.json" <<'PY'
import json,sys
path=sys.argv[1]; value=json.load(open(path)); value['registration_proposal']='record { action = opt variant { Motion = record { motion_text = "wrong" } } }'; json.dump(value,open(path,'w'))
PY
before="$(rg -c manage_neuron "$TRACE")"
if "$SUBMIT" "$T/bundle" "$MANIFEST_SHA" "$T/wrong-action.json" production \
  "$(printf 'c%.0s' {1..64})" aaaaa-aa "$T/preparation.json" "$T/reviewed.json" >/dev/null 2>&1; then
  echo "registration submission accepted a non-registration action" >&2; exit 1
fi
[[ "$before" == "$(rg -c manage_neuron "$TRACE")" && ! -e "$T/wrong-action.json" ]]
mv "$T/reviewed.valid.json" "$T/reviewed.json"

before="$(rg -c manage_neuron "$TRACE")"
if REGISTRATION_GATE_FAIL=true "$SUBMIT" "$T/bundle" "$MANIFEST_SHA" "$T/gate-failed.json" production \
  "$(printf 'c%.0s' {1..64})" aaaaa-aa "$T/preparation.json" "$T/reviewed.json" >/dev/null 2>&1; then
  echo "registration submission accepted an invalid preparation receipt" >&2; exit 1
fi
[[ "$before" == "$(rg -c manage_neuron "$TRACE")" && ! -e "$T/gate-failed.json" ]]

if "$SUBMIT" "$T/bundle" "$MANIFEST_SHA" "$T/submission.json" production \
  "$(printf 'c%.0s' {1..64})" aaaaa-aa "$T/preparation.json" "$T/reviewed.json" >/dev/null 2>&1; then
  echo "registration submission overwrote an existing receipt" >&2; exit 1
fi
[[ "$(rg -c manage_neuron "$TRACE")" == 1 ]]

if REGISTRATION_FAIL=true "$SUBMIT" "$T/bundle" "$MANIFEST_SHA" "$T/uncertain.json" production \
  "$(printf 'c%.0s' {1..64})" aaaaa-aa "$T/preparation.json" "$T/reviewed.json" >/dev/null 2>&1; then
  echo "registration submission accepted an uncertain proposal result" >&2; exit 1
fi
[[ -e "$T/uncertain.json" && -s "$T/uncertain.json.response.json" ]]
before="$(rg -c manage_neuron "$TRACE")"
if "$SUBMIT" "$T/bundle" "$MANIFEST_SHA" "$T/uncertain.json" production \
  "$(printf 'c%.0s' {1..64})" aaaaa-aa "$T/preparation.json" "$T/reviewed.json" >/dev/null 2>&1; then
  echo "registration submission retried an uncertain proposal" >&2; exit 1
fi
[[ "$before" == "$(rg -c manage_neuron "$TRACE")" ]]
