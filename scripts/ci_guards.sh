#!/usr/bin/env bash
# CI guards: centralize exact output checks so production gates and tests cannot drift.

readonly RUST_VERSION_PATTERN='^rustc 1\.97\.0([[:space:]]|$)'
readonly ICP_VERSION_PATTERN='^icp 1\.0\.2$'
readonly FOUNDRY_VERSION_PATTERN='^forge Version: 1\.7\.1$'
readonly ANVIL_VERSION_PATTERN='^anvil Version: 1\.7\.1$'
readonly Z3_VERSION_PATTERN='^Z3 version 4\.16\.0([[:space:]]|$)'
readonly VERUS_VERSION_PATTERN='^[[:space:]]*Version: 0\.2026\.07\.05\.49b8806$'
readonly LEAN_VERSION_PATTERN='^Lean \(version 4\.30\.0,'
readonly NODE_VERSION_PATTERN='^v24\.14\.0$'
readonly PNPM_VERSION_PATTERN='^11\.0\.8$'

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

verify_live_evm_rpc_rehearsal_sources() {
  if [[ "$#" -eq 0 ]]; then
    echo "no live EVM RPC rehearsal sources supplied" >&2
    return 1
  fi

  local source
  for source in "$@"; do
    [[ -f "$source" ]] || {
      echo "live EVM RPC rehearsal source not found: $source" >&2
      return 1
    }
  done

  if rg -n -i \
    'canister/mock-external|ui/e2e-real|https?://localhost|https?://127\.0\.0\.1|\b31337\b' \
    "$@"; then
    echo "live EVM RPC rehearsal source references a local or test-double backend" >&2
    return 1
  fi
  if ! rg -q '7hfb6-caaaa-aaaar-qadga-cai' "$@"; then
    echo "live EVM RPC rehearsal is not bound to the official EVM RPC Canister" >&2
    return 1
  fi
  if ! rg -q '84532' "$@"; then
    echo "live EVM RPC rehearsal is not bound to Base Sepolia" >&2
    return 1
  fi
  if ! rg -q 'capture-artifact' "$@" \
    || ! rg -q 'validate_raw_artifacts' "$@" \
    || ! rg -q 'CROSS_ARTIFACT_BINDINGS' "$@"; then
    echo "live EVM RPC rehearsal lacks raw artifact capture and cross-binding" >&2
    return 1
  fi
}

verify_no_obsolete_withdrawal_terms() {
  if [[ "$#" -eq 0 ]]; then
    echo "no protocol documentation supplied" >&2
    return 1
  fi
  if rg -n \
    '\bbeginRelease\b|canonical finalized|finalized (receipt|state|block|chain|Bridge event)' \
    "$@" --glob '*.md'; then
    echo "obsolete Withdrawal confirmation terminology found" >&2
    return 1
  fi
}

verify_lean_no_proof_escape() {
  if [[ "$#" -eq 0 ]]; then
    echo "no Lean proof source supplied" >&2
    return 1
  fi
  if rg -n '\b(sorry|admit)\b' "$@" --glob '*.lean'; then
    echo "forbidden Lean proof escape found" >&2
    return 1
  fi
}
