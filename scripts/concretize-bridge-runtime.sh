#!/usr/bin/env bash
# Produce the location-specific Bridge runtime by executing the reviewed constructors locally.
set -euo pipefail

SOURCE_ROOT="${1:?missing source root}"
PROFILE="${2:?missing profile}"
OUTPUT="${3:?missing runtime output}"
for tool in anvil cast forge python3; do command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }; done

read -r CHAIN DEPLOYER NONCE TIMELOCK BRIDGE BSNS DELAY PROPOSER CANCELLER EXECUTOR SIGNER ADMIN TL_HASH PER_DEPOSIT MINT_LIMIT MINT_WINDOW MIN_FEE MAX_FEE SERVICE_FEE < <(python3 - "$PROFILE" <<'PY'
import json,sys
p=json.load(open(sys.argv[1],encoding='utf-8')); d=p['initial_base_deployment']; t=p['timelock']; v=p['parameters']
print(p['chain_id'],d['deployer_address'],d['starting_nonce'],t['address'],p['bridge_contract'],p['bsns_contract'],t['minimum_delay_seconds'],t['proposer'],t['canceller'],t['executor'],p['expected_bridge_signer'],p['governance_operator'],t['runtime_code_hash'],v['per_deposit_limit'],v['mint_throughput_limit'],v['mint_window_duration_seconds'],v['min_service_fee'],v['max_service_fee'],v['service_fee'])
PY
)
PORT="$(python3 - <<'PY'
import socket
s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()
PY
)"
RPC="http://127.0.0.1:$PORT"
LOG="$(mktemp "${TMPDIR:-/tmp}/bridge-runtime-anvil.XXXXXX")"
anvil --silent --host 127.0.0.1 --port "$PORT" --chain-id "$CHAIN" --hardfork prague --timestamp 1 >"$LOG" 2>&1 &
ANVIL_PID=$!
cleanup() { kill "$ANVIL_PID" 2>/dev/null || true; wait "$ANVIL_PID" 2>/dev/null || true; rm -f "$LOG"; }
trap cleanup EXIT
for _ in {1..50}; do cast block-number --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 0.1; done
cast block-number --rpc-url "$RPC" >/dev/null
cast rpc --rpc-url "$RPC" anvil_impersonateAccount "$DEPLOYER" >/dev/null
cast rpc --rpc-url "$RPC" anvil_setBalance "$DEPLOYER" 0x3635c9adc5dea00000 >/dev/null
NONCE_HEX="$(python3 -c 'import sys;print(hex(int(sys.argv[1])))' "$NONCE")"
cast rpc --rpc-url "$RPC" anvil_setNonce "$DEPLOYER" "$NONCE_HEX" >/dev/null

FOUNDRY_PROFILE=default FOUNDRY_OFFLINE=true forge create --offline --broadcast --unlocked \
  --root "$SOURCE_ROOT/contracts" --rpc-url "$RPC" --chain "$CHAIN" --from "$DEPLOYER" --nonce "$NONCE" \
  src/BridgeTimelockController.sol:BridgeTimelockController \
  --constructor-args "$DELAY" "[$PROPOSER]" "[$CANCELLER]" "[$EXECUTOR]" >/dev/null
[[ "$(cast code "$TIMELOCK" --rpc-url "$RPC")" != 0x ]] || { echo "concrete Timelock address mismatch" >&2; exit 1; }

BRIDGE_NONCE="$((NONCE + 1))"
FOUNDRY_PROFILE=default FOUNDRY_OFFLINE=true forge create --offline --broadcast --unlocked \
  --root "$SOURCE_ROOT/contracts" --rpc-url "$RPC" --chain "$CHAIN" --from "$DEPLOYER" --nonce "$BRIDGE_NONCE" \
  src/Bridge.sol:Bridge --constructor-args "$SIGNER" "$ADMIN" "$TIMELOCK" "$TL_HASH" \
  "$PER_DEPOSIT" "$MINT_LIMIT" "$MINT_WINDOW" "$MIN_FEE" "$MAX_FEE" "$SERVICE_FEE" >/dev/null
RUNTIME="$(cast code "$BRIDGE" --rpc-url "$RPC")"
[[ "$RUNTIME" != 0x && "$(cast code "$BSNS" --rpc-url "$RPC")" != 0x ]] || {
  echo "concrete Bridge or bSNS address mismatch" >&2; exit 1;
}
python3 - "$RUNTIME" "$OUTPUT" <<'PY'
import os,pathlib,re,sys
value,target=sys.argv[1:]
if not re.fullmatch(r'0x[0-9a-fA-F]+',value) or len(value)%2: raise SystemExit('invalid concrete Bridge runtime')
path=pathlib.Path(target); path.write_bytes(bytes.fromhex(value[2:]));
with path.open('rb') as stream: os.fsync(stream.fileno())
PY
