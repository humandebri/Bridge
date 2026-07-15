#!/usr/bin/env bash
# CI guard tests: reject near-match versions and inconclusive SMT diagnostics.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=ci_guards.sh
source "$ROOT/scripts/ci_guards.sh"

TEST_TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/bridge-ci-guards.XXXXXX")"
trap 'rm -rf "$TEST_TMP_ROOT"' EXIT

expect_match() {
  local name="$1"
  local output="$2"
  local pattern="$3"

  if ! output_has_matching_line "$output" "$pattern"; then
    echo "expected matching output: $name" >&2
    return 1
  fi
}

expect_no_match() {
  local name="$1"
  local output="$2"
  local pattern="$3"

  if output_has_matching_line "$output" "$pattern"; then
    echo "unexpected matching output: $name" >&2
    return 1
  fi
}

expect_smt_counterexample() {
  local name="$1"
  local output="$2"

  if ! smt_output_has_counterexample "$output"; then
    echo "expected SMT counterexample: $name" >&2
    return 1
  fi
}

expect_no_smt_counterexample() {
  local name="$1"
  local output="$2"

  if smt_output_has_counterexample "$output"; then
    echo "unexpected SMT counterexample: $name" >&2
    return 1
  fi
}

expect_smt_fixture_gate_success() {
  local name="$1"
  local runner="$2"
  shift 2

  if ! verify_smt_failure_fixtures "$TEST_TMP_ROOT/$name" "$runner" "$@"; then
    echo "expected SMT fixture gate success: $name" >&2
    return 1
  fi
}

expect_smt_fixture_gate_failure() {
  local name="$1"
  local runner="$2"
  shift 2

  if verify_smt_failure_fixtures "$TEST_TMP_ROOT/$name" "$runner" "$@" >/dev/null 2>&1; then
    echo "expected SMT fixture gate failure: $name" >&2
    return 1
  fi
}

fake_counterexample_runner() {
  printf '%s\n' \
    'Warning (6328): CHC: Assertion violation happens here.' \
    'Counterexample:' \
    'value = 1'
  return 1
}

fake_mixed_runner() {
  local fixture="$1"

  if [[ "${fixture##*/}" == "accepted.sol" ]]; then
    echo "Compiler run successful"
    return 0
  fi
  fake_counterexample_runner
}

fake_inconclusive_runner() {
  echo "Warning: compilation failed"
  return 1
}

expect_match \
  "Rust release with commit metadata" \
  "rustc 1.97.0 (2d8144b78 2026-07-07)" \
  "$RUST_VERSION_PATTERN"
expect_match \
  "exact ICP CLI release" \
  "icp 1.0.2" \
  "$ICP_VERSION_PATTERN"
expect_no_match \
  "near-match ICP CLI release" \
  "icp 1.0.20" \
  "$ICP_VERSION_PATTERN"
expect_match \
  "Z3 release with platform metadata" \
  "Z3 version 4.16.0 - 64 bit" \
  "$Z3_VERSION_PATTERN"
expect_match \
  "exact Verus release" \
  $'Verus\n  Version: 0.2026.07.05.49b8806\n  Profile: release' \
  "$VERUS_VERSION_PATTERN"
expect_no_match \
  "suffixed Verus release" \
  "Version: 0.2026.07.05.49b8806-custom" \
  "$VERUS_VERSION_PATTERN"
expect_match \
  "exact Foundry release" \
  "forge Version: 1.7.1" \
  "$FOUNDRY_VERSION_PATTERN"
expect_no_match \
  "near-match Foundry release" \
  "forge Version: 1.7.10" \
  "$FOUNDRY_VERSION_PATTERN"
expect_no_match \
  "suffixed Foundry release" \
  "forge Version: 1.7.1-custom" \
  "$FOUNDRY_VERSION_PATTERN"
expect_no_match \
  "missing Foundry release" \
  "forge Version: unknown" \
  "$FOUNDRY_VERSION_PATTERN"
expect_match "exact Node.js release" "v24.14.0" "$NODE_VERSION_PATTERN"
expect_no_match "near-match Node.js release" "v24.14.1" "$NODE_VERSION_PATTERN"
expect_match "exact pnpm release" "11.0.8" "$PNPM_VERSION_PATTERN"
expect_no_match "near-match pnpm release" "11.0.80" "$PNPM_VERSION_PATTERN"

expect_smt_counterexample \
  "confirmed assertion violation" \
  $'Warning (6328): CHC: Assertion violation happens here.\nCounterexample:\nvalue = 1'
expect_no_smt_counterexample \
  "inconclusive assertion warning" \
  $'Warning (6328): CHC: Assertion violation might happen here.\nCounterexample:'
expect_no_smt_counterexample \
  "generic compiler warning" \
  "Warning: compilation failed"
expect_no_smt_counterexample "empty compiler output" ""

expect_smt_fixture_gate_success \
  "all-counterexamples" \
  fake_counterexample_runner \
  "first.sol" \
  "second.sol"
expect_smt_fixture_gate_failure \
  "one-fixture-accepted" \
  fake_mixed_runner \
  "failing.sol" \
  "accepted.sol"
expect_smt_fixture_gate_failure \
  "inconclusive-fixture" \
  fake_inconclusive_runner \
  "inconclusive.sol"
expect_smt_fixture_gate_failure "no-fixtures" fake_counterexample_runner

LIVE_REHEARSAL_SOURCE="$TEST_TMP_ROOT/live-rehearsal.py"
printf '%s\n' \
  'OFFICIAL_CANISTER = "7hfb6-caaaa-aaaar-qadga-cai"' \
  'CHAIN_ID = 84532' \
  'capture-artifact validate_raw_artifacts CROSS_ARTIFACT_BINDINGS' >"$LIVE_REHEARSAL_SOURCE"
verify_live_evm_rpc_rehearsal_sources "$LIVE_REHEARSAL_SOURCE"

LOCAL_REHEARSAL_SOURCE="$TEST_TMP_ROOT/local-rehearsal.py"
printf '%s\n' \
  'OFFICIAL_CANISTER = "7hfb6-caaaa-aaaar-qadga-cai"' \
  'CHAIN_ID = 84532' \
  'capture-artifact validate_raw_artifacts CROSS_ARTIFACT_BINDINGS' \
  'RPC = "http://localhost:8545"' >"$LOCAL_REHEARSAL_SOURCE"
if verify_live_evm_rpc_rehearsal_sources "$LOCAL_REHEARSAL_SOURCE" >/dev/null 2>&1; then
  echo "live EVM RPC guard accepted a local backend" >&2
  exit 1
fi

WRONG_CANISTER_SOURCE="$TEST_TMP_ROOT/wrong-canister.py"
printf '%s\n' 'OFFICIAL_CANISTER = "aaaaa-aa"' 'CHAIN_ID = 84532' \
  'capture-artifact validate_raw_artifacts CROSS_ARTIFACT_BINDINGS' >"$WRONG_CANISTER_SOURCE"
if verify_live_evm_rpc_rehearsal_sources "$WRONG_CANISTER_SOURCE" >/dev/null 2>&1; then
  echo "live EVM RPC guard accepted a non-official canister" >&2
  exit 1
fi
