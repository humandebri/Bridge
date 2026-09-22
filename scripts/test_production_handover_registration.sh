#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-handover-registration-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/source/scripts/candid" "$T/source/tools/sns-proposal" "$T/bundle"
cp "$ROOT/scripts/production-handover-registration-proposal.sh" "$T/source/scripts/"
cp "$ROOT/scripts/candid/kinic-sns-governance.did" "$T/source/scripts/candid/"
cp "$ROOT/tools/sns-proposal/handover.mjs" "$T/source/tools/sns-proposal/"
[[ "$(shasum -a 256 "$T/source/scripts/candid/kinic-sns-governance.did" | awk '{print $1}')" \
  == fa1d98d76edc1b09b70b39c7722291eb746006adefee03246bb5077a670fddae ]]
printf '\0asm\1\0\0\0test' >"$T/candidate.wasm"
CANDIDATE_SHA="$(shasum -a 256 "$T/candidate.wasm" | awk '{print $1}')"
cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"lb5i5-ziaaa-aaaar-qcgwq-cai","pause_principal":"lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe","root_canister_id":"7jkta-eyaaa-aaaaq-aaarq-cai","ic_host":"https://icp-api.io"}
JSON
cat >"$T/source/scripts/production-validation.sh" <<'SH'
production_require_clean_source(){ :; }
production_run_proof_gate(){ printf 'proof %s\n' "$*" >>"$TRACE"; [[ "${PROOF_FAIL:-false}" != true ]]; }
SH
cat >"$T/bin/git" <<'SH'
#!/usr/bin/env bash
if [[ "$*" == *'rev-parse HEAD'* ]]; then printf 'revision-1\n'
elif [[ "$*" == *'archive HEAD'* ]]; then printf 'tree\n'
else exit 0
fi
SH
cat >"$T/bin/cargo" <<'SH'
#!/usr/bin/env bash
mkdir -p "$CARGO_TARGET_DIR/release"
cat >"$CARGO_TARGET_DIR/release/bridge-profile" <<'INNER'
#!/usr/bin/env bash
printf 'verify %s\n' "$*" >>"$TRACE"
case "$1" in
  verify-production-current-state)
    case "$5" in joint-unregistered|root-registered) ;; *) exit 1 ;; esac
    ;;
  verify-sns-registration-live|verify-sns-upgrade-live) ;;
  *) exit 1 ;;
esac
INNER
chmod +x "$CARGO_TARGET_DIR/release/bridge-profile"
SH
cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf 'icp %s\n' "$*" >>"$TRACE"
if [[ "$*" == *'identity principal --identity production'* ]]; then
  printf 'lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe\n'
elif [[ "$*" == *'identity principal --identity llm-wiki-mainnet'* ]]; then
  printf 'r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae\n'
elif [[ "$*" == *'get_neuron'* ]]; then
  [[ "$*" == *"--candid $TEST_GOVERNANCE_CANDID"* ]] || {
    printf 'get_neuron omitted the reviewed Candid: %s\n' "$*" >&2; exit 98;
  }
  printf 'record { permissions = vec { record { "principal" = opt principal "r75h6-lqd7b-5jack-at55d-vvti2-lg5qy-ly73a-5ezve-odnkc-kagu3-nae"; permission_type = vec { 3 : int32; 4 : int32 } } } }\n'
elif [[ "$*" == *'status lb5i5-ziaaa-aaaar-qcgwq-cai'* ]]; then
  printf '{"module_hash":"0x%s"}\n' "$CANDIDATE_SHA"
elif [[ "$*" == *'build bridge-canister'* ]]; then
  mkdir -p "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release"
  cp "$TEST_SOURCE_WASM" "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/bridge_canister.wasm"
elif [[ "$*" == *'manage_neuron'* ]]; then
  [[ "$*" == *"--candid $TEST_GOVERNANCE_CANDID"* ]] || {
    printf 'manage_neuron omitted the reviewed Candid: %s\n' "$*" >&2; exit 98;
  }
  [[ "${SUBMIT_FAIL:-false}" != true ]] || exit 1
  printf '{"response_bytes":"00"}\n'
elif [[ "$*" == *'get_proposal'* ]]; then
  [[ "$*" == *"--candid $TEST_GOVERNANCE_CANDID"* ]] || {
    printf 'get_proposal omitted the reviewed Candid: %s\n' "$*" >&2; exit 98;
  }
  printf 'record { id = opt record { id = 42 : nat64 }; failed_timestamp_seconds = 0 : nat64; executed_timestamp_seconds = 1 : nat64; proposal = opt record { title = "Register KINIC Bridge with SNS"; action = opt variant { RegisterDappCanisters = record { canister_ids = vec { principal "lb5i5-ziaaa-aaaar-qcgwq-cai" } } } } }\n'
else
  printf '{"response_bytes":"00"}\n'
fi
SH
cat >"$T/bin/node" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *registration-payload*) printf 'record {title="Register KINIC Bridge with SNS";url="";summary="reviewed";action=opt variant {RegisterDappCanisters=record {canister_ids=vec {principal "lb5i5-ziaaa-aaaar-qcgwq-cai"}}}}\n' ;;
  *upgrade-payload*) printf 'record {title="Verify DAO upgrade of KINIC Bridge";url="";summary="reviewed";action=opt variant {UpgradeSnsControlledCanister=record {}}}\n' ;;
  *decode-chunk*) printf '%s\n' "$CANDIDATE_SHA" ;;
  *decode-stored-chunks*) printf '["%s"]\n' "$CANDIDATE_SHA" ;;
  *decode-response*) printf '42\n' ;;
  --check*) exit 0 ;;
  *) exit 1 ;;
esac
SH
chmod +x "$T/bin/"* "$T/source/scripts/production-handover-registration-proposal.sh"
export PATH="$T/bin:$PATH" TRACE="$T/trace" CANDIDATE_SHA TEST_SOURCE_WASM="$T/candidate.wasm"
TEST_GOVERNANCE_CANDID="$(cd "$T/source/scripts/candid" && pwd)/kinic-sns-governance.did"
export TEST_GOVERNANCE_CANDID
export BRIDGE_RELEASE_BUNDLE="$T/bundle" BRIDGE_ICP_IDENTITY=production
DRIVER="$T/source/scripts/production-handover-registration-proposal.sh"

"$DRIVER" check-registration --wasm "$T/candidate.wasm" >"$T/check-registration.out"
rg -q 'kind=registration' "$T/check-registration.out"
rg -q 'joint-unregistered' "$TRACE"

cp "$TEST_GOVERNANCE_CANDID" "$T/reviewed-governance.did"
printf '\n' >>"$TEST_GOVERNANCE_CANDID"
if "$DRIVER" check-registration --wasm "$T/candidate.wasm" >/dev/null 2>"$T/candid-drift.err"; then
  echo "handover check accepted Governance Candid drift" >&2; exit 1
fi
rg -q 'Governance Candid differs from the reviewed interface' "$T/candid-drift.err"
mv "$T/reviewed-governance.did" "$TEST_GOVERNANCE_CANDID"

if "$DRIVER" execute-registration --wasm "$T/candidate.wasm" \
  --expected-current-wasm "$CANDIDATE_SHA" >/dev/null 2>&1; then
  echo "registration accepted missing confirmation" >&2; exit 1
fi
BRIDGE_CONFIRM_SNS_DAPP_REGISTRATION=REGISTER_PRODUCTION_BRIDGE_WITH_KINIC_SNS \
  "$DRIVER" execute-registration --wasm "$T/candidate.wasm" \
  --expected-current-wasm "$CANDIDATE_SHA" >"$T/registration.out"
rg -q 'proposal_id=42' "$T/registration.out"
[[ "$(rg -c 'manage_neuron' "$TRACE")" == 1 ]]
[[ "$(rg -c 'upload_chunk' "$TRACE")" == 1 ]]

"$DRIVER" check-upgrade --wasm "$T/candidate.wasm" --registration-proposal-id 42 >"$T/check-upgrade.out"
rg -q 'kind=upgrade' "$T/check-upgrade.out"
rg -q 'root-registered' "$TRACE"

BRIDGE_CONFIRM_SNS_SAME_WASM_UPGRADE=UPGRADE_REGISTERED_BRIDGE_WITH_SAME_WASM \
  "$DRIVER" execute-upgrade --wasm "$T/candidate.wasm" \
  --expected-current-wasm "$CANDIDATE_SHA" --registration-proposal-id 42 >"$T/upgrade.out"
rg -q 'proposal_id=42' "$T/upgrade.out"
[[ "$(rg -c 'manage_neuron' "$TRACE")" == 2 ]]

"$DRIVER" verify-upgrade --wasm "$T/candidate.wasm" \
  --registration-proposal-id 42 --upgrade-proposal-id 43 >"$T/verify-upgrade.out"
rg -q 'verify-sns-upgrade-live .*candidate.wasm 42 43' "$TRACE"
[[ "$(rg -c 'manage_neuron' "$TRACE")" == 2 ]]

before="$(rg -c 'manage_neuron' "$TRACE")"
if PROOF_FAIL=true "$DRIVER" check-registration --wasm "$T/candidate.wasm" >/dev/null 2>&1; then
  echo "proposal check accepted a failed proof gate" >&2; exit 1
fi
[[ "$before" == "$(rg -c 'manage_neuron' "$TRACE")" ]]

if SUBMIT_FAIL=true BRIDGE_CONFIRM_SNS_SAME_WASM_UPGRADE=UPGRADE_REGISTERED_BRIDGE_WITH_SAME_WASM \
  "$DRIVER" execute-upgrade --wasm "$T/candidate.wasm" \
  --expected-current-wasm "$CANDIDATE_SHA" --registration-proposal-id 42 >/dev/null 2>"$T/uncertain.err"; then
  echo "uncertain proposal submission succeeded" >&2; exit 1
fi
rg -q 'do not resubmit for 6 minutes' "$T/uncertain.err"
