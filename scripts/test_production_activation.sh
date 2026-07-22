#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-activation-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT
mkdir -p "$T/bin" "$T/bundle"
TRACE="$T/trace"
: >"$TRACE"

cat >"$T/bin/icp" <<'SH'
#!/usr/bin/env bash
printf 'icp %s\n' "$*" >>"$TRACE"
if [[ "$1 $2" == "identity principal" ]]; then
  echo '2vxsx-fae'
elif [[ "$*" == *list_nervous_system_functions* ]]; then
  cat <<'JSON'
{"functions":[{"id":101,"function_type":{"GenericNervousSystemFunction":{"target_canister_id":"aaaaa-aa","target_method_name":"schedule_activation"}}}]}
JSON
elif [[ "$*" == *manage_neuron* ]]; then
  date +%s >"$MANAGE_NEURON_AT"
  sleep 2
  echo '{"command":{"MakeProposal":{"proposal_id":{"id":42}}}}'
else
  exit 1
fi
SH
chmod +x "$T/bin/icp"

cat >"$T/bundle/profile.json" <<'JSON'
{"bridge_canister_id":"aaaaa-aa"}
JSON
cat >"$T/bundle/release-manifest.json" <<'JSON'
{"release_id":"release-test","source_revision":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_tree_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}
JSON

PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(printf 'c%.0s' {1..64})" "$T/submission.json" proposer \
  "$(printf 'd%.0s' {1..64})" 2vxsx-fae >/dev/null

python3 - "$T/submission.json" "$T/manage-neuron-at" <<'PY'
import json,sys
value=json.load(open(sys.argv[1],encoding='utf-8'))
assert value['schema_version']==3
assert value['phase']=='schedule'
assert value['proposal_id']==42
assert value['function_id']==101
assert value['target_method_name']=='schedule_activation'
assert value['payload_hex']=='4449444c0000'
assert value['proposer_principal']=='2vxsx-fae'
assert value['submitted_at_unix'] <= int(open(sys.argv[2],encoding='utf-8').read())
PY
[[ "$(grep -c manage_neuron "$TRACE")" -eq 1 ]]

if PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(printf 'c%.0s' {1..64})" "$T/submission.json" proposer \
  "$(printf 'd%.0s' {1..64})" 2vxsx-fae >/dev/null 2>&1; then
  echo "activation proposal accepted a reused checkpoint path" >&2
  exit 1
fi
[[ "$(grep -c manage_neuron "$TRACE")" -eq 1 ]]

if PATH="$T/bin:$PATH" TRACE="$TRACE" MANAGE_NEURON_AT="$T/manage-neuron-at" "$ROOT/scripts/production-activation-proposal.sh" \
  schedule "$T/bundle" "$(printf 'c%.0s' {1..64})" "$T/rejected.json" proposer \
  "$(printf 'd%.0s' {1..64})" aaaaa-aa >/dev/null 2>&1; then
  echo "activation proposal accepted an identity/principal mismatch" >&2
  exit 1
fi
[[ -e "$T/rejected.json" && ! -s "$T/rejected.json" ]]
[[ "$(grep -c manage_neuron "$TRACE")" -eq 1 ]]

cat >"$T/bin/cast" <<'SH'
#!/usr/bin/env bash
printf 'cast %s\n' "$*" >>"$TRACE"
case "$1" in
  chain-id) echo 8453 ;;
  block) printf '{"number":"0x64","hash":"0x%s"}\n' "$(printf 'a%.0s' {1..64})" ;;
  calldata) echo 0x1234 ;;
  rpc)
    [[ "$*" == *'"blockHash":"0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","requireCanonical":true'* ]] || exit 1
    printf '"0x%s"\n' "$(printf '0%.0s' {1..63})1"
    ;;
  *) exit 1 ;;
esac
SH
chmod +x "$T/bin/cast"
cat >"$T/bundle/profile.json" <<'JSON'
{"chain_id":8453,"bridge_contract":"0x1111111111111111111111111111111111111111","timelock":{"address":"0x2222222222222222222222222222222222222222"},"rpc_providers":[{"url":"https://rpc1.invalid"},{"url":"https://rpc2.invalid"},{"url":"https://rpc3.invalid"}]}
JSON
: >"$TRACE"
PATH="$T/bin:$PATH" TRACE="$TRACE" "$ROOT/scripts/production-live-preflight.sh" \
  verify-activation schedule "$T/bundle" "0x$(printf '3%.0s' {1..64})" >/dev/null
[[ "$(grep -c '^cast rpc ' "$TRACE")" -eq 9 ]]
! grep -q '^cast call ' "$TRACE"
