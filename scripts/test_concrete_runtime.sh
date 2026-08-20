#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
T="$(mktemp -d "${TMPDIR:-/tmp}/bridge-concrete-runtime.XXXXXX")"
trap 'rm -rf "$T"' EXIT
DEPLOYER=0x0000000000000000000000000000000000000007
TIMELOCK="$(cast compute-address "$DEPLOYER" --nonce 0 | awk '{print $NF}')"
BRIDGE="$(cast compute-address "$DEPLOYER" --nonce 1 | awk '{print $NF}')"
BSNS="$(cast compute-address "$BRIDGE" --nonce 1 | awk '{print $NF}')"
TIMELOCK_RUNTIME="$(FOUNDRY_PROFILE=default FOUNDRY_OFFLINE=true forge inspect --offline --root "$ROOT/contracts" src/BridgeTimelockController.sol:BridgeTimelockController deployedBytecode)"
TIMELOCK_HASH="$(cast keccak "$TIMELOCK_RUNTIME")"
python3 - "$T/profile.json" "$DEPLOYER" "$TIMELOCK" "$BRIDGE" "$BSNS" "$TIMELOCK_HASH" <<'PY'
import json,sys
target,deployer,timelock,bridge,bsns,timelock_hash=sys.argv[1:]
value={
 "chain_id":8453,"bridge_contract":bridge,"bsns_contract":bsns,
 "expected_bridge_signer":"0x0000000000000000000000000000000000000002",
 "governance_operator":"0x0000000000000000000000000000000000000003",
 "initial_base_deployment":{"deployer_address":deployer,"starting_nonce":0},
 "timelock":{"address":timelock,"runtime_code_hash":timelock_hash,"minimum_delay_seconds":86400,
             "proposer":"0x0000000000000000000000000000000000000003",
             "canceller":"0x0000000000000000000000000000000000000003",
             "executor":"0x0000000000000000000000000000000000000003"},
 "parameters":{"per_deposit_limit":"1","mint_throughput_limit":"1","mint_window_duration_seconds":"3600",
               "max_service_fee":"1000000000","service_fee":"50000000"}}
with open(target,'w',encoding='utf-8') as out: json.dump(value,out)
PY
bash "$ROOT/scripts/concretize-bridge-runtime.sh" "$ROOT" "$T/profile.json" "$T/first.bin"
bash "$ROOT/scripts/concretize-bridge-runtime.sh" "$ROOT" "$T/profile.json" "$T/second.bin"
cmp "$T/first.bin" "$T/second.bin"
[[ -s "$T/first.bin" ]]
python3 - "$T/profile.json" <<'PY'
import json,sys
path=sys.argv[1]; value=json.load(open(path)); value['bsns_contract']='0x0000000000000000000000000000000000000099'
with open(path,'w') as out: json.dump(value,out)
PY
if bash "$ROOT/scripts/concretize-bridge-runtime.sh" "$ROOT" "$T/profile.json" "$T/rejected.bin" >/dev/null 2>&1; then
  echo "concrete runtime generation accepted a mismatched bSNS address" >&2
  exit 1
fi
