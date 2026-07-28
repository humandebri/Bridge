#!/usr/bin/env bash
# CI guard tests: reject near-match versions and inconclusive SMT diagnostics.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=ci_guards.sh
source "$ROOT/scripts/ci_guards.sh"

if rg -n 'frontend-sepolia|assetstorage|certified-assets' "$ROOT/icp.yaml"; then
  echo "IC configuration must not deploy the Cloudflare-hosted frontend as an Asset Canister" >&2
  exit 1
fi
node - "$ROOT/ui/package.json" <<'JS'
const fs = require("node:fs")
const command = JSON.parse(fs.readFileSync(process.argv[2], "utf8")).scripts?.["deploy:test"]
const requiredSteps = [
  "pnpm run build:sepolia",
  "node scripts/check-sepolia-assets.mjs",
  "wrangler deploy --name kinic-bridge-ui-test",
]
const steps = typeof command === "string" ? command.split(/\s*&&\s*/) : []
let cursor = -1
for (const step of requiredSteps) {
  const index = steps.indexOf(step, cursor + 1)
  if (index === -1) {
    throw new Error(`test frontend deploy command is missing the ordered step: ${step}`)
  }
  cursor = index
}
JS

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
  "exact Lean release" \
  "Lean (version 4.30.0, arm64-apple-darwin24.6.0, Release)" \
  "$LEAN_VERSION_PATTERN"
expect_no_match \
  "near-match Lean release" \
  "Lean (version 4.30.1, arm64-apple-darwin24.6.0, Release)" \
  "$LEAN_VERSION_PATTERN"
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

CURRENT_PROTOCOL_DOC="$TEST_TMP_ROOT/current-protocol.md"
printf '%s\n' \
  'The user createWithdrawal transaction burns bSNS and enters Committed.' \
  'The canister binds all reads to one canonical Safe block.' >"$CURRENT_PROTOCOL_DOC"
verify_no_obsolete_withdrawal_terms "$CURRENT_PROTOCOL_DOC"

OBSOLETE_PROTOCOL_DOC="$TEST_TMP_ROOT/obsolete-protocol.md"
printf '%s\n' 'The canister calls beginRelease before a canonical finalized receipt.' \
  >"$OBSOLETE_PROTOCOL_DOC"
if verify_no_obsolete_withdrawal_terms "$OBSOLETE_PROTOCOL_DOC" >/dev/null 2>&1; then
  echo "protocol documentation guard accepted the retired Withdrawal flow" >&2
  exit 1
fi

COMPLETE_LEAN_SOURCE="$TEST_TMP_ROOT/Complete.lean"
printf '%s\n' 'theorem complete : True := by trivial' >"$COMPLETE_LEAN_SOURCE"
verify_lean_no_proof_escape "$COMPLETE_LEAN_SOURCE"

INCOMPLETE_LEAN_SOURCE="$TEST_TMP_ROOT/Incomplete.lean"
printf '%s\n' 'theorem incomplete : True := by sorry' >"$INCOMPLETE_LEAN_SOURCE"
if verify_lean_no_proof_escape "$INCOMPLETE_LEAN_SOURCE" >/dev/null 2>&1; then
  echo "Lean proof guard accepted sorry" >&2
  exit 1
fi

CI_LOCAL_SOURCE="$ROOT/scripts/ci-local.sh"
if ! rg -q '^run_smoke_step\(\)' "$CI_LOCAL_SOURCE" ||
  ! rg -q '^  trap cleanup_runtime EXIT$' "$CI_LOCAL_SOURCE"; then
  echo "local smoke must clean up resources inside the run_step subshell" >&2
  exit 1
fi
if rg -q '^    run_step smoke run_smoke$' "$CI_LOCAL_SOURCE"; then
  echo "local smoke bypasses its subshell cleanup wrapper" >&2
  exit 1
fi

if ! rg -q 'mkdir\(path\.dirname\(outputPath\), \{ recursive: true \}\)' \
  "$ROOT/scripts/plan007/generate-local-e2e.mjs"; then
  echo "local E2E evidence generator does not create its output directory" >&2
  exit 1
fi
