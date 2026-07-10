#!/usr/bin/env bash
# Repository CI entrypoint: run deterministic Phase 0 checks and isolated local deploy smoke tests.
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
  local output
  local address
  local code

  output="$(forge create \
    --root "$CONTRACTS" \
    --rpc-url http://127.0.0.1:8545 \
    --from 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 \
    --unlocked \
    --broadcast \
    "$identifier")"
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
  echo "$identifier deployed at $address"
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

  deploy_contract "src/BSNS.sol:BSNS"
  deploy_contract "src/Bridge.sol:Bridge"
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
