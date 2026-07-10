#!/usr/bin/env bash
# Repository CI entrypoint: run deterministic checks and isolated local deploy smoke tests.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACTS="$ROOT/contracts"
MODE="${1:-all}"
export PATH="$ROOT/.tools/bin:$PATH"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/bridge-phase0.XXXXXX")"
ANVIL_PID=""
ICP_NETWORK_OWNED=0
ICP_CONFIG_BACKED_UP=0
CLEANUP_DONE=0

# shellcheck source=ci_guards.sh
source "$ROOT/scripts/ci_guards.sh"

cleanup() {
  if [[ "$CLEANUP_DONE" -eq 1 ]]; then
    return
  fi
  CLEANUP_DONE=1
  trap - EXIT INT TERM

  if [[ -n "$ANVIL_PID" ]] && kill -0 "$ANVIL_PID" 2>/dev/null; then
    kill "$ANVIL_PID"
    wait "$ANVIL_PID" 2>/dev/null || true
  fi
  if [[ "$ICP_NETWORK_OWNED" -eq 1 ]]; then
    icp network stop --project-root-override "$ROOT" >/dev/null 2>&1 || true
  fi

  if [[ "$ICP_CONFIG_BACKED_UP" -eq 1 ]]; then
    if cmp -s "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.original"; then
      :
    elif cmp -s "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.applied"; then
      cp -p "$TMP_ROOT/icp.yaml.original" "$ROOT/icp.yaml"
    else
      echo "icp.yaml changed during smoke; preserving the current file" >&2
    fi
  fi
  rm -rf "$TMP_ROOT"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_versions() {
  "$ROOT/scripts/check_tool_versions.sh"
  "$ROOT/scripts/test_ci_guards.sh"
}

run_rust() {
  cargo fmt --manifest-path "$ROOT/Cargo.toml" --all --check
  cargo clippy --manifest-path "$ROOT/Cargo.toml" --workspace --all-targets -- -D warnings
  cargo test --manifest-path "$ROOT/Cargo.toml" --workspace
  cargo build \
    --manifest-path "$ROOT/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release \
    -p bridge-canister
  python3 "$ROOT/scripts/test_prepare_local_network.py"
}

run_contracts() {
  forge fmt --root "$CONTRACTS" --check
  forge build --root "$CONTRACTS"
  forge test --root "$CONTRACTS"
}

run_smt() {
  local failure_log="$TMP_ROOT/smt-failure.log"
  local failure_status

  FOUNDRY_PROFILE=smt forge build \
    --root "$CONTRACTS" \
    --contracts "$ROOT/verification/smt/pass" \
    --skip test \
    --skip script \
    --force

  set +e
  FOUNDRY_PROFILE=smt forge build \
    --root "$CONTRACTS" \
    --contracts "$ROOT/verification/smt/fail" \
    --skip test \
    --skip script \
    --force >"$failure_log" 2>&1
  failure_status=$?
  set -e

  if [[ "$failure_status" -eq 0 ]]; then
    echo "SMTChecker accepted the deliberate failing fixture" >&2
    cat "$failure_log" >&2
    return 1
  fi
  if ! smt_output_has_counterexample "$(<"$failure_log")"; then
    echo "SMTChecker failed without reporting the expected assertion counterexample" >&2
    cat "$failure_log" >&2
    return 1
  fi
}

run_verus() {
  local failure_log="$TMP_ROOT/verus-failure.log"
  local failure_status

  verus --no-cheating "$ROOT/verification/verus/pass.rs" -o "$TMP_ROOT/verus-pass"

  set +e
  verus --no-cheating "$ROOT/verification/verus/fail.rs" \
    -o "$TMP_ROOT/verus-fail" >"$failure_log" 2>&1
  failure_status=$?
  set -e

  if [[ "$failure_status" -eq 0 ]]; then
    echo "Verus accepted the deliberate failing fixture" >&2
    cat "$failure_log" >&2
    return 1
  fi
  if ! rg -qi "postcondition.*not satisfied|postcondition.*fail" "$failure_log"; then
    echo "Verus failed without reporting the expected postcondition violation" >&2
    cat "$failure_log" >&2
    return 1
  fi
}

run_proofs() {
  run_smt
  run_verus
}

run_icp_build() {
  icp project show --project-root-override "$ROOT" >/dev/null
  icp build bridge-canister --project-root-override "$ROOT"
}

wait_for_anvil() {
  local attempt
  for attempt in {1..50}; do
    if cast chain-id --rpc-url http://127.0.0.1:8545 >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.2
  done
  echo "Anvil did not become ready" >&2
  return 1
}

deploy_contract() {
  local identifier="$1"
  shift
  local output
  local address
  local code
  local command=(
    forge create
    --root "$CONTRACTS"
    --rpc-url http://127.0.0.1:8545
    --from 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
    --unlocked
    --broadcast
    "$identifier"
  )

  if [[ "$#" -gt 0 ]]; then
    command+=(--constructor-args "$@")
  fi
  output="$("${command[@]}")"
  address="$(printf '%s\n' "$output" | sed -n 's/^Deployed to: //p' | tail -n 1)"
  if [[ -z "$address" ]]; then
    echo "could not parse deployed address for $identifier" >&2
    echo "$output" >&2
    return 1
  fi

  code="$(cast code "$address" --rpc-url http://127.0.0.1:8545)"
  if [[ "$code" == "0x" || -z "$code" ]]; then
    echo "deployed contract has no runtime bytecode: $identifier at $address" >&2
    return 1
  fi
  echo "$identifier deployed at $address" >&2
  printf '%s\n' "$address"
}

require_equal() {
  local label="$1"
  local actual="$2"
  local expected="$3"

  if [[ "$actual" != "$expected" ]]; then
    echo "$label mismatch: expected $expected, got $actual" >&2
    return 1
  fi
}

prepare_temporary_icp_config() {
  cp -p "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.original"
  ICP_CONFIG_BACKED_UP=1
  "$ROOT/scripts/prepare_local_network.py" --project-root "$ROOT" --write >/dev/null
  cp -p "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.applied"
}

ensure_icp_network() {
  local network_status="$1"

  if icp network status --json --project-root-override "$ROOT" \
    >"$network_status" 2>/dev/null; then
    return
  fi

  prepare_temporary_icp_config
  # Mark ownership before start so cleanup also handles signals during startup.
  ICP_NETWORK_OWNED=1
  icp network start -d --project-root-override "$ROOT"
  icp network status --json --project-root-override "$ROOT" >"$network_status"
}

run_smoke() {
  local network_status="$TMP_ROOT/icp-network-status.json"
  local canister_status="$TMP_ROOT/canister-status.json"
  local bridge_address
  local bsns_address
  local token_bridge
  local token_name
  local token_symbol
  local token_version
  local token_decimals
  local recipient_balance
  local processed
  local readonly bridge_signer="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
  local readonly runtime_administrator="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
  local readonly base_admin_timelock="0x90F79bf6EB2c4f870365E785982E1f101E93b906"
  local readonly recipient="0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65"
  local readonly deposit_id="0x0000000000000000000000000000000000000000000000000000000000000001"
  local readonly gross_amount="101000000"
  local readonly service_fee="1000000"
  local readonly minted_amount="100000000"

  ensure_icp_network "$network_status"
  icp deploy -e local --project-root-override "$ROOT"
  icp canister status bridge-canister -e local --json \
    --project-root-override "$ROOT" >"$canister_status"
  if ! rg -qi '"status"[[:space:]]*:[[:space:]]*"running"' "$canister_status"; then
    echo "bridge-canister is not running" >&2
    cat "$canister_status" >&2
    return 1
  fi

  if cast chain-id --rpc-url http://127.0.0.1:8545 >/dev/null 2>&1; then
    echo "port 8545 is already serving an EVM node; refusing to reuse it" >&2
    return 1
  fi
  anvil --chain-id 31337 --silent >"$TMP_ROOT/anvil.log" 2>&1 &
  ANVIL_PID=$!
  wait_for_anvil
  if [[ "$(cast chain-id --rpc-url http://127.0.0.1:8545)" != "31337" ]]; then
    echo "unexpected Anvil chain ID" >&2
    return 1
  fi

  bridge_address="$(deploy_contract \
    "src/Bridge.sol:Bridge" \
    "kinic" \
    "KINIC" \
    "8" \
    "$bridge_signer" \
    "$runtime_administrator" \
    "$base_admin_timelock" \
    "1000000000000" \
    "10000000000000" \
    "3600" \
    "100000000" \
    "$service_fee")"
  bsns_address="$(cast call "$bridge_address" "bsns()(address)" --rpc-url http://127.0.0.1:8545)"
  if [[ "$(cast code "$bsns_address" --rpc-url http://127.0.0.1:8545)" == "0x" ]]; then
    echo "Bridge-created bSNS has no runtime bytecode: $bsns_address" >&2
    return 1
  fi

  token_bridge="$(cast call "$bsns_address" "bridge()(address)" --rpc-url http://127.0.0.1:8545)"
  require_equal \
    "bSNS Bridge binding" \
    "$(printf '%s' "$token_bridge" | tr '[:upper:]' '[:lower:]')" \
    "$(printf '%s' "$bridge_address" | tr '[:upper:]' '[:lower:]')"
  token_name="$(cast call "$bsns_address" "name()(string)" --rpc-url http://127.0.0.1:8545)"
  token_symbol="$(cast call "$bsns_address" "symbol()(string)" --rpc-url http://127.0.0.1:8545)"
  token_version="$(cast call "$bsns_address" "version()(string)" --rpc-url http://127.0.0.1:8545)"
  read -r token_decimals _ <<<"$(cast call "$bsns_address" "decimals()(uint8)" --rpc-url http://127.0.0.1:8545)"
  require_equal "bSNS name" "$token_name" '"kinic"'
  require_equal "bSNS symbol" "$token_symbol" '"KINIC"'
  require_equal "bSNS EIP-712 version" "$token_version" '"1"'
  require_equal "bSNS decimals" "$token_decimals" "8"

  cast send \
    "$bridge_address" \
    "mintDeposit((bytes32,address,uint256,uint256))" \
    "($deposit_id,$recipient,$gross_amount,$service_fee)" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$bridge_signer" \
    --unlocked >/dev/null
  read -r recipient_balance _ <<<"$(
    cast call "$bsns_address" "balanceOf(address)(uint256)" "$recipient" --rpc-url http://127.0.0.1:8545
  )"
  processed="$(
    cast call "$bridge_address" "isDepositProcessed(bytes32)(bool)" "$deposit_id" \
      --rpc-url http://127.0.0.1:8545
  )"
  require_equal "smoke recipient balance" "$recipient_balance" "$minted_amount"
  require_equal "smoke Deposit processed state" "$processed" "true"
  echo "Bridge-created bSNS deployed at $bsns_address" >&2
}

run_checks() {
  run_versions
  run_rust
  run_contracts
  run_proofs
  run_icp_build
}

case "$MODE" in
  all)
    run_checks
    run_smoke
    ;;
  checks)
    run_checks
    ;;
  versions)
    run_versions
    ;;
  rust)
    run_rust
    ;;
  contracts)
    run_contracts
    ;;
  proofs)
    run_proofs
    ;;
  icp)
    run_icp_build
    ;;
  smoke)
    run_smoke
    ;;
  *)
    echo "usage: $0 {all|checks|versions|rust|contracts|proofs|icp|smoke}" >&2
    exit 2
    ;;
esac
