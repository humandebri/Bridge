#!/usr/bin/env bash
# Phase 0 CI guards: centralize exact output checks so production gates and tests cannot drift.

readonly RUST_VERSION_PATTERN='^rustc 1\.93\.0([[:space:]]|$)'
readonly ICP_VERSION_PATTERN='^icp 0\.2\.7$'
readonly FOUNDRY_VERSION_PATTERN='^forge Version: 1\.7\.1$'
readonly ANVIL_VERSION_PATTERN='^anvil Version: 1\.7\.1$'
readonly Z3_VERSION_PATTERN='^Z3 version 4\.15\.4([[:space:]]|$)'
readonly VERUS_VERSION_PATTERN='^[[:space:]]*Version: 0\.2026\.05\.05\.d03e906$'

output_has_matching_line() {
  local output="$1"
  local pattern="$2"

  rg -q -- "$pattern" <<<"$output"
}

smt_output_has_counterexample() {
  local output="$1"

  output_has_matching_line \
    "$output" \
    '^Warning \(6328\): CHC: Assertion violation happens here\.$' &&
    output_has_matching_line "$output" '^Counterexample:$'
}
