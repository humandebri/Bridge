#!/usr/bin/env bash
# CI guards: centralize exact output checks so production gates and tests cannot drift.

readonly RUST_VERSION_PATTERN='^rustc 1\.97\.0([[:space:]]|$)'
readonly ICP_VERSION_PATTERN='^icp 1\.0\.2$'
readonly FOUNDRY_VERSION_PATTERN='^forge Version: 1\.7\.1$'
readonly ANVIL_VERSION_PATTERN='^anvil Version: 1\.7\.1$'
readonly Z3_VERSION_PATTERN='^Z3 version 4\.16\.0([[:space:]]|$)'
readonly VERUS_VERSION_PATTERN='^[[:space:]]*Version: 0\.2026\.07\.05\.49b8806$'

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

verify_smt_failure_fixtures() {
  local log_dir="$1"
  local runner="$2"
  shift 2

  if [[ "$#" -eq 0 ]]; then
    echo "no SMT failing fixtures found" >&2
    return 1
  fi

  mkdir -p "$log_dir"

  local fixture
  local fixture_index=0
  local fixture_name
  local log_index
  local failure_log
  local failure_status
  for fixture in "$@"; do
    fixture_index=$((fixture_index + 1))
    fixture_name="${fixture##*/}"
    log_index="$(printf '%03d' "$fixture_index")"
    failure_log="$log_dir/$log_index-${fixture_name%.sol}.log"

    if "$runner" "$fixture" >"$failure_log" 2>&1; then
      failure_status=0
    else
      failure_status=$?
    fi

    if [[ "$failure_status" -eq 0 ]]; then
      echo "SMTChecker accepted the deliberate failing fixture: $fixture" >&2
      cat "$failure_log" >&2
      return 1
    fi
    if ! smt_output_has_counterexample "$(<"$failure_log")"; then
      echo "SMTChecker failed without the expected assertion counterexample: $fixture" >&2
      cat "$failure_log" >&2
      return 1
    fi
  done
}
