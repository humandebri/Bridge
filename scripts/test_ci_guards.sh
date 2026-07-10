#!/usr/bin/env bash
# CI guard tests: reject near-match versions and inconclusive SMT diagnostics.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=ci_guards.sh
source "$ROOT/scripts/ci_guards.sh"

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
