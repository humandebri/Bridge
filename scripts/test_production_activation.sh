#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-activation-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/bundle"
TRACE="$T/trace"
: >"$TRACE"

export REGISTRY="$T/registry.json"
export PROPOSAL_RESPONSE="$T/proposal-response.json"
cd "$ROOT"
[[ "$(node --version)" == "v$(<.node-version)" ]] || { echo 'Node version mismatch' >&2; exit 1; }
# A failing fnm stub catches accidental reintroduction of the manager dependency.
printf '#!/bin/sh\nexit 99\n' >"$T/bin/fnm"
chmod +x "$T/bin/fnm"
export PATH="$T/bin:$PATH"
node --test tools/sns-proposal/prepare.test.mjs
node --input-type=module >"$REGISTRY" <<'JS'
import { IDL } from '@icp-sdk/core/candid';
import { writeFileSync } from 'node:fs';
import { Principal } from '@icp-sdk/core/principal';
const generic = IDL.Record({target_canister_id: IDL.Opt(IDL.Principal), target_method_name: IDL.Opt(IDL.Text), validator_canister_id: IDL.Opt(IDL.Principal), validator_method_name: IDL.Opt(IDL.Text)});
const type = IDL.Record({reserved_ids: IDL.Vec(IDL.Nat64), functions: IDL.Vec(IDL.Record({id: IDL.Nat64, function_type: IDL.Opt(IDL.Variant({GenericNervousSystemFunction: generic}))}))});
const bridge = Principal.fromText('lb5i5-ziaaa-aaaar-qcgwq-cai');
const bytes = IDL.encode([type], [{reserved_ids: [], functions: [{id: 101n, function_type: [{GenericNervousSystemFunction: {target_canister_id:[bridge],target_method_name:['sns_schedule_activation'],validator_canister_id:[bridge],validator_method_name:['validate_sns_schedule_activation']}}]}]}]);
const responseType = IDL.Record({command: IDL.Opt(IDL.Variant({MakeProposal: IDL.Record({proposal_id: IDL.Opt(IDL.Record({id: IDL.Nat64}))})}))});
writeFileSync(process.env.PROPOSAL_RESPONSE, JSON.stringify({response_bytes: Buffer.from(IDL.encode([responseType],[{command:[{MakeProposal:{proposal_id:[{id:42n}]}}]}])).toString('hex')}));
console.log(JSON.stringify({response_bytes: Buffer.from(bytes).toString('hex')}));
JS
node tools/sns-proposal/prepare.mjs "$REGISTRY" lb5i5-ziaaa-aaaar-qcgwq-cai 7 "$T/reviewed.json"

cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf 'icp %s\n' "$*" >>"$TRACE"
if [[ "$1 $2" == "identity principal" ]]; then
  echo '2vxsx-fae'
elif [[ "$*" == *list_nervous_system_functions* ]]; then
  cat "$REGISTRY"
elif [[ "$*" == *manage_neuron* ]]; then
  date +%s >"$MANAGE_NEURON_AT"
  sleep 2
  cat "$PROPOSAL_RESPONSE"
else
  exit 1
fi
SH
chmod +x "$T/bin/icp"

cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"lb5i5-ziaaa-aaaar-qcgwq-cai"}
JSON
cat >"$T/bundle/release-manifest.json" <<'JSON'
{"release_id":"release-test","source_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_tree_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
JSON

PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')" "$T/submission.json" proposer \
  "$(printf 'd%.0s' {1..64})" 2vxsx-fae 7 "$T/reviewed.json" >/dev/null

python3 - "$T/submission.json" "$T/manage-neuron-at" <<'PY'
import json,sys
value=json.load(open(sys.argv[1],encoding='utf-8'))
assert value['schema_version']==4
assert value['phase']=='schedule'
assert value['proposal_id']==42
assert value['function_id']==101
assert value['target_method_name']=='sns_schedule_activation'
assert value['previous_governance_operation_id']==7
assert value['validator_method_name']=='validate_sns_schedule_activation'
assert value['payload_hex'].startswith('4449444c')
assert value['payload_hex']!='4449444c0000'
assert value['proposer_principal']=='2vxsx-fae'
assert value['submitted_at_unix'] <= int(open(sys.argv[2],encoding='utf-8').read())
PY
[[ "$(grep -c manage_neuron "$TRACE")" -eq 1 ]]

if PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')" "$T/submission.json" proposer \
  "$(printf 'd%.0s' {1..64})" 2vxsx-fae 7 "$T/reviewed.json" >/dev/null 2>&1; then
  echo "activation proposal accepted a reused checkpoint path" >&2
  exit 1
fi
[[ "$(grep -c manage_neuron "$TRACE")" -eq 1 ]]

if PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')" "$T/rejected.json" proposer \
  "$(printf 'd%.0s' {1..64})" aaaaa-aa 7 "$T/reviewed.json" >/dev/null 2>&1; then
  echo "activation proposal accepted an identity/principal mismatch" >&2
  exit 1
fi
[[ ! -e "$T/rejected.json" ]]
[[ "$(grep -c manage_neuron "$TRACE")" -eq 1 ]]

echo "production activation proposal tests passed"

python3 - "$T/reviewed.json" "$T/drifted.json" "$T/submission.json.response.json" <<'PY_TEST'
import json,sys
value=json.load(open(sys.argv[1]));value['proposals'][0]['function_id']='9999'
json.dump(value,open(sys.argv[2],'w'))
assert json.load(open(sys.argv[3]))['exit_code']==0
PY_TEST
if PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')" "$T/drifted-submission.json" proposer \
  "$(printf 'd%.0s' {1..64})" 2vxsx-fae 7 "$T/drifted.json" >/dev/null 2>&1; then
  echo "activation proposal accepted a different reviewed function ID" >&2; exit 1
fi
[[ "$(rg -c manage_neuron "$TRACE")" -eq 1 ]]

mkdir "$T/wrong-node"
printf '#!/bin/sh\necho v0.0.0\n' >"$T/wrong-node/node"
chmod +x "$T/wrong-node/node"
cp "$TRACE" "$T/trace-before"
if PATH="$T/wrong-node:$PATH" TRACE="$TRACE" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(shasum -a 256 "$T/bundle/release-manifest.json" | awk '{print $1}')" "$T/wrong-node.json" proposer \
  "$(printf 'd%.0s' {1..64})" 2vxsx-fae 7 "$T/reviewed.json" >"$T/wrong-node.log" 2>&1; then
  echo "activation proposal accepted the wrong Node version" >&2; exit 1
fi
grep -q 'Node on PATH must match' "$T/wrong-node.log"
cmp "$TRACE" "$T/trace-before"
[[ ! -e "$T/wrong-node.json" && ! -e "$T/wrong-node.json.response.json" ]]
