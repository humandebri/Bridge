#!/usr/bin/env bash
# Repository CI entrypoint: run deterministic checks and isolated local deploy smoke tests.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTRACTS="$ROOT/contracts"
MODE="${1:-all}"

if [[ "${BRIDGE_CI_LOCAL_NODE_REEXEC:-0}" != 1 ]]; then
  EXPECTED_NODE_VERSION="v$(<"$ROOT/.node-version")"
  CURRENT_NODE_VERSION="$(node --version 2>/dev/null || true)"
  if [[ "$CURRENT_NODE_VERSION" != "$EXPECTED_NODE_VERSION" ]] \
    && command -v fnm >/dev/null 2>&1; then
    echo "ci-local: selecting Node.js $EXPECTED_NODE_VERSION with fnm" >&2
    exec fnm exec --using "$EXPECTED_NODE_VERSION" env \
      BRIDGE_CI_LOCAL_NODE_REEXEC=1 "$ROOT/scripts/ci-local.sh" "$@"
  fi
fi

export PATH="$ROOT/.tools/bin:$PATH"
TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/bridge-phase0.XXXXXX")"
ANVIL_PID=""
ICP_NETWORK_OWNED=0
ICP_CONFIG_BACKED_UP=0
ICP_TEST_CANISTER_CREATED=0
ICP_TEST_CANISTER_SNAPSHOT=""
ICP_TEST_CANISTER_WAS_RUNNING=0
ICP_TEST_CANISTER_INSTALL_MODE="install"
ICP_LOCAL_MAPPING_BACKED_UP=0
ICP_SMOKE_STATE_PREPARED=0
CLEANUP_DONE=0
UI_DEPENDENCIES_READY=0

# shellcheck source=ci_guards.sh
# shellcheck source=ci_guards.sh
source "$ROOT/scripts/ci_guards.sh"

cleanup_runtime() {
  local cleanup_failed=0
  if [[ -n "$ANVIL_PID" ]] && kill -0 "$ANVIL_PID" 2>/dev/null; then
    kill "$ANVIL_PID"
    wait "$ANVIL_PID" 2>/dev/null || true
    ANVIL_PID=""
  fi
  if [[ "$ICP_NETWORK_OWNED" -eq 0 ]] && declare -F restore_smoke_canister_state >/dev/null; then
    restore_smoke_canister_state || cleanup_failed=1
  fi
  if [[ "$ICP_NETWORK_OWNED" -eq 1 ]]; then
    if icp network stop --project-root-override "$ROOT" >/dev/null 2>&1; then
      rm -f "$ROOT/.icp/cache/networks/local/bridge-ci-project-marker.json" || cleanup_failed=1
      ICP_NETWORK_OWNED=0
    else
      echo "failed to stop the Bridge-owned local ICP network; ownership marker retained" >&2
      cleanup_failed=1
    fi
  fi

  if [[ "$ICP_CONFIG_BACKED_UP" -eq 1 ]]; then
    if cmp -s "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.original"; then
      ICP_CONFIG_BACKED_UP=0
    elif cmp -s "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.applied"; then
      if cp -p "$TMP_ROOT/icp.yaml.original" "$ROOT/icp.yaml"; then
        ICP_CONFIG_BACKED_UP=0
      else
        cleanup_failed=1
      fi
    else
      echo "icp.yaml changed during smoke; preserving the current file" >&2
      ICP_CONFIG_BACKED_UP=0
    fi
  fi
  if [[ "$cleanup_failed" -ne 0 ]]; then
    echo "local CI cleanup was incomplete; retained snapshots and current state require manual inspection" >&2
    return 1
  fi
  return 0
}

cleanup() {
  local original_status=$?
  if [[ "$CLEANUP_DONE" -eq 1 ]]; then
    exit "$original_status"
  fi
  CLEANUP_DONE=1
  trap - EXIT INT TERM

  local cleanup_failed=0
  cleanup_runtime || cleanup_failed=1
  if [[ "$cleanup_failed" -ne 0 ]]; then
    echo "local CI cleanup failed; recovery artifacts retained at $TMP_ROOT" >&2
    if [[ "$original_status" -eq 0 ]]; then exit 1; else exit "$original_status"; fi
  fi
  rm -rf "$TMP_ROOT" || {
    echo "local CI temporary directory cleanup failed: $TMP_ROOT" >&2
    if [[ "$original_status" -eq 0 ]]; then exit 1; else exit "$original_status"; fi
  }
  exit "$original_status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_step() {
  local name="$1"
  shift
  local stderr_file="$TMP_ROOT/${name//[^a-zA-Z0-9_-]/_}.stderr"
  echo "==> $name" >&2
  set +e
  ( set -e; "$@" ) 2> >(tee "$stderr_file" >&2)
  local status=$?
  set -e
  if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
    {
      printf '### `%s` — exit `%d`\n\n' "$name" "$status"
      if [[ -s "$stderr_file" ]]; then
        printf '<details><summary>stderr (last 80 lines)</summary>\n\n~~~text\n'
        tail -n 80 "$stderr_file"
        printf '~~~\n</details>\n\n'
      else
        printf '_No stderr output._\n\n'
      fi
    } >>"$GITHUB_STEP_SUMMARY"
  fi
  return "$status"
}

run_certora_advisory_checks() {
  shellcheck -x -S warning \
    "$ROOT/scripts/install-certora-solc.sh" \
    "$ROOT/scripts/run_certora_advisory.sh"
  forge build --root "$ROOT/contracts"
  python3 "$ROOT/scripts/check_certora_manifest.py"
  python3 "$ROOT/scripts/test_certora_fingerprint.py"
  python3 "$ROOT/scripts/test_certora_manifest.py"
  python3 "$ROOT/scripts/test_certora_results.py"
}

run_versions() {
  python3 "$ROOT/scripts/trusted_proof_profiles.py" --print >/dev/null
  verify_no_npm_lockfiles "$ROOT"
  shellcheck -x -S warning \
    "$ROOT/scripts/ci-local.sh" \
    "$ROOT/scripts/production-release.sh" \
    "$ROOT/scripts/production-validation.sh" \
    "$ROOT/scripts/production-deploy-driver.sh" \
    "$ROOT/scripts/production-activation-proposal.sh" \
    "$ROOT/scripts/production-activate-driver.sh" \
    "$ROOT/scripts/production-handover-driver.sh" \
    "$ROOT/scripts/install-certora-solc.sh" \
    "$ROOT/scripts/trusted-pr-container.sh"
  "$ROOT/scripts/check_tool_versions.sh"
  "$ROOT/scripts/test_tool_version_gate.sh"
  python3 "$ROOT/scripts/check_schema_consistency.py"
  python3 "$ROOT/scripts/check_no_obsolete_release_dependencies.py"
  python3 "$ROOT/scripts/test_no_obsolete_release_dependencies.py"
  verify_no_obsolete_withdrawal_terms \
    "$ROOT/README.md" "$ROOT/docs" "$ROOT/verification"
  python3 "$ROOT/scripts/check_sqlite_transaction_boundaries.py"
  python3 "$ROOT/scripts/test_ci_changed_areas.py"
  forge build --root "$ROOT/contracts" --ast
  python3 "$ROOT/scripts/check_certora_manifest.py"
  python3 "$ROOT/scripts/test_certora_manifest.py"
  python3 "$ROOT/scripts/test_proof_impact.py"
  python3 "$ROOT/scripts/test_trusted_proof_profiles.py"
  python3 "$ROOT/scripts/test_trusted_dependency_profiles.py"
  python3 "$ROOT/scripts/test_proof_fingerprint_candidate_scripts.py"
  python3 "$ROOT/scripts/test_ci_modes.py"
  python3 "$ROOT/scripts/test_trusted_pr_gate.py"
  "$ROOT/scripts/test_ci_guards.sh"
  "$ROOT/scripts/test_production_canister_bootstrap.sh"
  "$ROOT/scripts/test_production_release.sh"
  "$ROOT/scripts/test_production_drivers.sh"
  "$ROOT/scripts/test_production_activation.sh"
  "$ROOT/scripts/test_production_handover.sh"
  python3 "$ROOT/scripts/evm-rpc-rehearsal/test_rehearsal.py"
  python3 "$ROOT/scripts/plan007/test_sepolia_e2e.py"
  node "$ROOT/scripts/plan007/test-check-upgrade-instance.mjs"
  node "$ROOT/scripts/plan007/test-read-public-canister-metadata.mjs"
  python3 "$ROOT/scripts/plan007/test_candid_values.py"
  python3 "$ROOT/scripts/plan007/test_staging_wasm_artifact.py"
  python3 "$ROOT/scripts/plan007/test_staging_canister_upgrade.py"
  python3 "$ROOT/scripts/plan007/test_fault_injector.py"
  verify_live_evm_rpc_rehearsal_sources \
    "$ROOT/scripts/evm-rpc-rehearsal/rehearsal.py"
}

verify_tecdsa_wrapper_dependency() {
  python3 - "$ROOT" <<'PY'
import sys
import tomllib
from pathlib import Path

root = Path(sys.argv[1])
expected_version = "=0.1.1"
expected_source = "registry+https://github.com/rust-lang/crates.io-index"
expected_checksum = "92c1319a274caebf0ab70ab826b8905c29e8563498289356b9a59464f2a85c56"

with (root / "Cargo.toml").open("rb") as source:
    workspace = tomllib.load(source)
dependency = workspace.get("workspace", {}).get("dependencies", {}).get(
    "ic-cdk-management-canister"
)
if dependency != expected_version:
    raise SystemExit("workspace must pin ic-cdk-management-canister = \"=0.1.1\"")

patches = workspace.get("patch", {})
for registry, entries in patches.items():
    if not isinstance(entries, dict):
        continue
    for name, value in entries.items():
        package = value.get("package") if isinstance(value, dict) else None
        if name == "ic-cdk-management-canister" or package == "ic-cdk-management-canister":
            raise SystemExit(f"tECDSA wrapper dependency must not be patched through {registry}")

with (root / "canister" / "bridge-canister" / "Cargo.toml").open("rb") as source:
    canister = tomllib.load(source)
if canister.get("dependencies", {}).get("ic-cdk-management-canister") != {
    "workspace": True
}:
    raise SystemExit("bridge-canister must use the pinned workspace tECDSA wrapper")

with (root / "Cargo.lock").open("rb") as source:
    lockfile = tomllib.load(source)
packages = [
    package
    for package in lockfile.get("package", [])
    if package.get("name") == "ic-cdk-management-canister"
]
if len(packages) != 1:
    raise SystemExit("Cargo.lock must contain exactly one tECDSA wrapper package")
package = packages[0]
if (
    package.get("version") != expected_version.removeprefix("=")
    or package.get("source") != expected_source
    or package.get("checksum") != expected_checksum
):
    raise SystemExit("Cargo.lock does not bind the reviewed tECDSA wrapper artifact")
PY
}

run_no_automatic_execution_guards() {
  verify_tecdsa_wrapper_dependency
  if rg -n '\b(set_timer_interval|heartbeat)\b' \
    "$ROOT/canister/bridge-canister/src"; then
    echo "recurring canister execution path found" >&2
    return 1
  fi
  if rg -n '\bunbounded_wait\b' \
    "$ROOT/canister/bridge-canister/src" --glob '*.rs'; then
    echo "unbounded canister execution path found" >&2
    return 1
  fi
  if rg -n '\bextern\b' \
    "$ROOT/canister/bridge-canister/src" --glob '*.rs'; then
    echo "extern declarations may rebind the reviewed tECDSA wrapper" >&2
    return 1
  fi
  local signing_reference_count
  local signing_method_string_count
  signing_reference_count="$(
    rg -o '\bsign_with_ecdsa\b' \
      "$ROOT/canister/bridge-canister/src" --glob '*.rs' \
      | wc -l \
      | tr -d ' ' \
      || true
  )"
  signing_method_string_count="$(
    rg -o '"sign_with_ecdsa"' \
      "$ROOT/canister/bridge-canister/src" --glob '*.rs' \
      | wc -l \
      | tr -d ' ' \
      || true
  )"
  if (( signing_method_string_count > signing_reference_count )); then
    echo "threshold signing reference count is inconsistent" >&2
    return 1
  fi
  signing_reference_count=$((signing_reference_count - signing_method_string_count))
  if [[ "$signing_reference_count" != "1" ]] \
    || ! rg -q \
      '^[[:space:]]*::ic_cdk_management_canister::sign_with_ecdsa\(sign_args\)[[:space:]]*$' \
      "$ROOT/canister/bridge-canister/src/signer.rs"; then
    echo "threshold signing must contain exactly one fully qualified reviewed wrapper call" >&2
    return 1
  fi
  if rg -n '\bset_timer\b' \
    "$ROOT/canister/bridge-canister/src" \
    --glob '!scheduler.rs'; then
    echo "one-shot timer found outside the stable settlement executor" >&2
    return 1
  fi
  if rg -n '\b(scheduler_priority|scheduler_code|candidate_precedes)\b' \
    "$ROOT/canister/bridge-core" "$ROOT/canister/bridge-canister/src" "$ROOT/verification/verus"; then
    echo "retired scheduler implementation found" >&2
    return 1
  fi
  if rg -n '\b(sessionStorage|localStorage)\b' "$ROOT/ui/src" \
    --glob '!pending-confirmations.ts' \
    --glob '!pending-confirmations.test.ts' \
    --glob '!deposit-intents.ts' \
    --glob '!deposit-intents.test.ts' \
    --glob '!browser-lock.ts' \
    --glob '!browser-lock.test.ts' \
    --glob '!settlement-confirmation-coordinator.tsx' \
    --glob '!settlement-confirmation-coordinator.test.tsx' \
    --glob '!settlement-confirmation-recovery.test.tsx' \
    --glob '!risk-acknowledgement.tsx' \
    --glob '!risk-acknowledgement.test.tsx'; then
    echo "browser storage is used outside the reviewed recovery modules" >&2
    return 1
  fi
  if rg -n '\b(getTransactionReceipt|waitForTransactionReceipt)\b' "$ROOT/ui/src/routes/history.tsx"; then
    echo "browser-side withdrawal receipt precheck found" >&2
    return 1
  fi
}

require_workspace_dependencies() {
  if [[ "${BRIDGE_TRUSTED_DEPS_READY:-0}" == "1" ]]; then
    if [[ ! -d "$ROOT/node_modules" || -L "$ROOT/node_modules" ]]; then
      echo "trusted workspace dependencies are missing or symlinked" >&2
      return 1
    fi
  elif [[ "${CI:-}" == "true" ]]; then
    pnpm --dir "$ROOT" install --frozen-lockfile
  elif [[ ! -d "$ROOT/node_modules" ]]; then
    echo "node_modules is missing; run pnpm install --frozen-lockfile before checks" >&2
    return 1
  fi
}

run_rust_fast() {
  run_no_automatic_execution_guards
  cargo fmt --manifest-path "$ROOT/Cargo.toml" --all --check
  cargo clippy --locked --manifest-path "$ROOT/Cargo.toml" --workspace --all-targets -- -D warnings
  cargo clippy --locked --manifest-path "$ROOT/Cargo.toml" -p bridge-canister --all-targets \
    --features test-deployment -- -D warnings
  cargo test --locked --manifest-path "$ROOT/Cargo.toml" --workspace
}

run_rust_integration() {
  cargo build \
    --locked \
    --manifest-path "$ROOT/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release \
    -p bridge-canister
  cargo build \
    --locked \
    --manifest-path "$ROOT/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release \
    -p mock-external
  require_workspace_dependencies
  pnpm --dir "$ROOT" run governance-relayer:test
  pnpm --dir "$ROOT" run governance-relayer:typecheck
  pnpm --dir "$ROOT" run integration:typecheck
  pnpm --dir "$ROOT" run test:e2e
  python3 "$ROOT/scripts/test_prepare_local_network.py"
  bash "$ROOT/scripts/test_ci_local_safety.sh"
  node "$ROOT/scripts/plan007/test-generate-local-e2e.mjs"
}

run_rust() {
  run_rust_fast
  run_rust_integration
}

run_contracts_fast() {
  forge fmt --root "$CONTRACTS" --check
  forge build --root "$CONTRACTS" --sizes --ignored-error-codes 2394 --ignored-error-codes 3860 --ignored-error-codes 6335
  python3 "$ROOT/scripts/abi_snapshot.py" --check
  forge test --root "$CONTRACTS"
}

run_contracts_coverage() {
  forge coverage \
    --root "$CONTRACTS" \
    --ir-minimum \
    --report summary \
    --no-match-coverage 'BridgeTimelockController\.sol' \
    --ignored-error-codes 2394 \
    --ignored-error-codes 6335 \
    --ignored-error-codes 3860 \
    --ignored-error-codes 5574
}

run_contracts() {
  run_contracts_fast
  run_contracts_coverage
}

build_smt_failure_fixture() {
  local fixture="$1"
  local candidate
  local -a skip_args=()

  while IFS= read -r candidate; do
    skip_args+=(--skip "$(basename "$candidate")")
  done < <(rg --files "$ROOT/verification/smt/pass" -g '*.sol' | sort)

  while IFS= read -r candidate; do
    if [[ "$candidate" != "$fixture" ]]; then
      skip_args+=(--skip "$(basename "$candidate")")
    fi
  done < <(rg --files "$ROOT/verification/smt/fail" -g '*.sol' | sort)

  forge build \
    --root "$ROOT/verification/smt" \
    "${skip_args[@]}" \
    --force

}

run_smt() {
  local -a failure_fixtures=()
  local -a pass_skip_args=()
  local failure_fixture

  python3 "$ROOT/scripts/smt_obligations.py" --check-sources

  while IFS= read -r failure_fixture; do
    failure_fixtures+=("$failure_fixture")
    pass_skip_args+=(--skip "$(basename "$failure_fixture")")
  done < <(rg --files "$ROOT/verification/smt/fail" -g '*.sol' | sort)

  forge build \
    --root "$ROOT/verification/smt" \
    "${pass_skip_args[@]}" \
    --force

  python3 "$ROOT/scripts/check_solidity_ast_bindings.py" --scope smt

  verify_smt_failure_fixtures \
    "$TMP_ROOT/smt-failures" \
    build_smt_failure_fixture \
    "${failure_fixtures[@]}"
}

run_verus() {
  local failure_fixture
  local failure_log
  local failure_status
  local kind
  local kernel_name
  local proof_name
  local expected_fixture
  local verus_version

  verus_version="$(verus --version 2>&1)"
  if ! output_has_matching_line "$verus_version" "$VERUS_VERSION_PATTERN"; then
    echo "verus version mismatch for proofs" >&2
    echo "$verus_version" >&2
    return 1
  fi

  if rg -n '\b(assume|admit|external_body)\b' \
    "$ROOT/canister/bridge-core/src/kernel.rs" \
    "$ROOT/verification/verus/pass.rs" \
    "$ROOT/verification/verus/fail"; then
    echo "forbidden Verus proof escape found" >&2
    return 1
  fi

  verus --no-cheating "$ROOT/verification/verus/pass.rs" -o "$TMP_ROOT/verus-pass"
  python3 "$ROOT/scripts/check_verus_manifest.py"

  while IFS=$'\t' read -r obligation_id kind kernel_name proof_name expected_fixture _binding _derived_bindings _production_calls _claim_ids; do
    [[ "$obligation_id" == "schema" ]] && continue
    case "$kind" in
      shared-expression|derived|model)
        rg -q "pub open spec fn ${kernel_name}_spec\b" "$ROOT/canister/bridge-core/src/kernel.rs" || {
          echo "Verus manifest spec is missing: $kernel_name" >&2
          return 1
        }
        rg -q "proof fn ${proof_name}\b" "$ROOT/verification/verus/pass.rs" || {
          echo "Verus manifest proof is missing: $proof_name" >&2
          return 1
        }
        rg -q "${kernel_name}_spec\b" "$ROOT/verification/verus/fail/$expected_fixture" || {
          echo "Verus failure fixture does not reference ${kernel_name}_spec: $expected_fixture" >&2
          return 1
        }
        rg -q "pub (const )?fn ${kernel_name}\b" "$ROOT/canister/bridge-core/src/kernel.rs" || {
          [[ "$kind" == "model" ]] || {
            echo "non-model Verus kernel is missing: $kernel_name" >&2
            return 1
          }
        }
        ;;
      executable)
        rg -q "^fn ${proof_name}\b" "$ROOT/verification/verus/pass.rs" || {
          echo "Verus executable obligation is missing: $proof_name" >&2
          return 1
        }
        rg -q "kernel::${kernel_name}\b" "$ROOT/verification/verus/pass.rs" || {
          echo "Verus executable obligation does not call production symbol: $kernel_name" >&2
          return 1
        }
        rg -q "kernel::${kernel_name}\b" "$ROOT/verification/verus/fail/$expected_fixture" || {
          echo "Verus failure fixture does not call production symbol: $expected_fixture" >&2
          return 1
        }
        rg -q "pub fn ${kernel_name}\b" "$ROOT/canister/bridge-core/src/kernel.rs" || {
          echo "Verus executable kernel is missing: $kernel_name" >&2
          return 1
        }
        ;;
      *)
        echo "invalid Verus manifest kind: $kind" >&2
        return 1
        ;;
    esac
  done < "$ROOT/verification/verus/manifest.tsv"

  while IFS= read -r failure_fixture; do
    failure_log="$TMP_ROOT/verus-$(basename "$failure_fixture" .rs).log"
    set +e
    verus --no-cheating "$failure_fixture" \
      -o "$TMP_ROOT/verus-$(basename "$failure_fixture" .rs)" >"$failure_log" 2>&1
    failure_status=$?
    set -e
    if [[ "$failure_status" -eq 0 ]]; then
      echo "Verus accepted deliberate failing fixture: $failure_fixture" >&2
      return 1
    fi
    if ! rg -qi "postcondition.*not satisfied|postcondition.*fail" "$failure_log"; then
      echo "Verus fixture failed without expected postcondition violation: $failure_fixture" >&2
      cat "$failure_log" >&2
      return 1
    fi
  done < <(rg --files "$ROOT/verification/verus/fail" -g '*.rs' | sort)
}

run_halmos() {
  local halmos_entry="$ROOT/verification/halmos/.venv/bin/halmos"
  local halmos_python="$ROOT/verification/halmos/.venv/bin/python"
  local -a halmos
  local row_type
  local obligation_id
  local positive_link
  local positive_path
  local positive_contract
  local positive_function
  local positive_log
  local failure_id
  local failure_link
  local failure_path
  local failure_contract
  local failure_function
  local failure_log
  local failure_status
  local version

  python3 "$ROOT/scripts/halmos_obligations.py" --check-sources

  if [[ ! -x "$halmos_entry" || ! -x "$halmos_python" ]]; then
    echo "missing locked Halmos environment; run uv sync --project verification/halmos --frozen" >&2
    return 1
  fi
  python3 "$ROOT/scripts/halmos_environment.py" --check || return
  halmos=("$halmos_python" "$halmos_entry")
  version="$("${halmos[@]}" --version 2>&1)"
  if [[ "$version" != "halmos 0.3.3" ]]; then
    echo "Halmos version mismatch for proofs: $version" >&2
    return 1
  fi

  while IFS=$'\t' read -r row_type obligation_id _strength positive_link \
      _production_link _failure_ids _claim_ids; do
    [[ "$row_type" == "schema" ]] && continue
    positive_path="${positive_link%%#*}"
    positive_function="${positive_link#*#}"
    positive_contract="$(basename "$positive_path" .t.sol)"
    positive_log="$TMP_ROOT/halmos-positive-$obligation_id.log"
    if ! "${halmos[@]}" \
      --root "$ROOT/contracts" \
      --contract "$positive_contract" \
      --function "$positive_function" \
      --solver z3 \
      --solver-timeout-assertion 30s \
      --early-exit \
      --no-status >"$positive_log" 2>&1; then
      echo "Halmos positive obligation failed: $obligation_id" >&2
      cat "$positive_log" >&2
      return 1
    fi
    if ! rg -q 'Symbolic test result: 1 passed; 0 failed' "$positive_log" \
      || rg -qi '\[FAIL\]|counterexample|timeout|unknown' "$positive_log"; then
      echo "Halmos positive obligation did not complete exactly: $obligation_id" >&2
      cat "$positive_log" >&2
      return 1
    fi
  done < "$ROOT/verification/halmos/obligations.tsv"
  python3 "$ROOT/scripts/check_solidity_ast_bindings.py" --scope bridge || return

  while IFS=$'\t' read -r failure_id failure_link; do
    failure_path="${failure_link%%#*}"
    failure_function="${failure_link#*#}"
    failure_contract="$(basename "$failure_path" .t.sol)Halmos"
    failure_log="$TMP_ROOT/halmos-$failure_id.log"
    set +e
    "${halmos[@]}" \
      --root "$ROOT/contracts" \
      --contract "$failure_contract" \
      --function "$failure_function" \
      --solver z3 \
      --solver-timeout-assertion 30s \
      --early-exit \
      --no-status >"$failure_log" 2>&1
    failure_status=$?
    set -e
    if [[ "$failure_status" -eq 0 ]] \
      || ! rg -q '\[FAIL\]' "$failure_log" \
      || ! rg -qi 'counterexample' "$failure_log" \
      || ! rg -q 'Symbolic test result: 0 passed; 1 failed' "$failure_log" \
      || rg -qi 'timeout|unknown' "$failure_log"; then
      echo "Halmos negative fixture did not produce one counterexample: $failure_id" >&2
      cat "$failure_log" >&2
      return 1
    fi
  done < "$ROOT/verification/halmos/failure-manifest.tsv"
}

run_lean_failure_fixtures() {
  local failure_fixture
  local failure_log
  local failure_status

  while IFS= read -r failure_fixture; do
    failure_log="$TMP_ROOT/lean-$(basename "$failure_fixture" .lean).log"
    set +e
    (cd "$ROOT/verification/lean" && lake env lean "$failure_fixture") \
      >"$failure_log" 2>&1
    failure_status=$?
    set -e
    if [[ "$failure_status" -eq 0 ]]; then
      echo "Lean accepted deliberate failing fixture: $failure_fixture" >&2
      return 1
    fi
    if ! rg -qi "proved that the proposition" "$failure_log" \
      || ! rg -q "^is false$" "$failure_log"; then
      echo "Lean fixture failed without the expected false theorem rejection: $failure_fixture" >&2
      cat "$failure_log" >&2
      return 1
    fi
  done < <(rg --files "$ROOT/verification/lean/fail" -g '*.lean' | sort)
}

run_lean_proofs() {
  verify_lean_no_proof_escape "$ROOT/verification/lean"
  (cd "$ROOT/verification/lean" && lake build)
}

run_policy_vector_consumers() {
  python3 "$ROOT/scripts/test_protocol_vectors.py" || return
  python3 "$ROOT/scripts/protocol_vectors.py" --check
}

run_refinement_gate() {
  python3 "$TRANSITION_MANIFEST_TEST" || return
  python3 "$TRANSITION_MANIFEST_CHECK" || return
  python3 "$ROOT/scripts/test_reproducible_artifacts.py" || return
  python3 "$REFINEMENT_MANIFEST_TEST" || return
  python3 "$REFINEMENT_GENERATOR" --check || return
  python3 "$REFINEMENT_MANIFEST_CHECK" || return
  python3 "$PROOF_IMPACT_CHECK"
}

run_proof_stage() {
  local stage="$1"
  shift
  local status
  local stage_status
  set +e
  (
    set -e
    python3 "$PROOF_FINGERPRINT" --check "$PROOF_SOURCE_BASELINE" >/dev/null || {
      echo "proof source fingerprint changed before stage: $stage" >&2
      exit 1
    }
    if ! "$@"; then
      echo "proof stage command failed: $stage" >&2
      exit 1
    fi
    python3 "$PROOF_FINGERPRINT" --check "$PROOF_SOURCE_BASELINE" >/dev/null || {
      echo "proof source fingerprint changed during stage: $stage" >&2
      exit 1
    }
  )
  status=$?
  set -e
  if [[ "$status" -eq 0 ]]; then
    stage_status=pass
  else
    stage_status=fail
  fi
  printf '%s\t%s\t' "$stage" "$stage_status" >>"$PROOF_STAGE_RECEIPT"
  tr -d '\n' <"$PROOF_SOURCE_BASELINE" >>"$PROOF_STAGE_RECEIPT"
  printf '\n' >>"$PROOF_STAGE_RECEIPT"
  if ! python3 "$PROOF_RECEIPT_WRITER" \
    "$PROOF_STAGE_RECEIPT" "$PROOF_RECEIPT" "$PROOF_SOURCE_BASELINE"; then
    echo "proof receipt write failed after stage: $stage" >&2
    return 1
  fi
  return "$status"
}

run_proofs() {
  python3 "$ROOT/scripts/trusted_proof_profiles.py" --print >/dev/null
  CLAIM_CHECK="$ROOT/scripts/check_claim_manifest.py"
  CLAIM_TEST_CHECK="$ROOT/scripts/check_claim_test_manifest.py"
  CLAIM_TEST_TEST="$ROOT/scripts/test_claim_test_manifest.py"
  CONCRETE_RUNTIME_TEST="$ROOT/scripts/test_concrete_runtime.sh"
  FAILURE_MANIFEST_CHECK="$ROOT/scripts/check_failure_manifests.py"
  PROOF_IMPACT_CHECK="$ROOT/scripts/check_proof_impact.py"
  PROOF_FINGERPRINT="$ROOT/scripts/proof_fingerprint.py"
  PROOF_RECEIPT_WRITER="$ROOT/scripts/write_proof_receipt.py"
  REFINEMENT_GENERATOR="$ROOT/scripts/generate_refinement_harness.py"
  REFINEMENT_MANIFEST_CHECK="$ROOT/scripts/check_refinement_manifest.py"
  REFINEMENT_MANIFEST_TEST="$ROOT/scripts/test_refinement_manifest.py"
  TRANSITION_MANIFEST_CHECK="$ROOT/scripts/check_transition_manifest.py"
  TRANSITION_MANIFEST_TEST="$ROOT/scripts/test_transition_manifest.py"
  PROOF_STAGE_RECEIPT="$TMP_ROOT/proof-stages.tsv"
  PROOF_SOURCE_BASELINE="$TMP_ROOT/proof-source-fingerprint.json"
  PROOF_RECEIPT="${PROOF_RECEIPT:-$ROOT/verification/output/proof-receipt.json}"
  : >"$PROOF_STAGE_RECEIPT"
  python3 "$PROOF_FINGERPRINT" --write "$PROOF_SOURCE_BASELINE" >/dev/null
  python3 "$CLAIM_CHECK" >/dev/null
  python3 "$ROOT/scripts/test_write_proof_receipt.py"
  python3 "$CLAIM_TEST_TEST"
  python3 "$ROOT/scripts/test_check_claim_manifest.py"
  python3 "$ROOT/scripts/test_halmos_environment.py"
  python3 "$ROOT/scripts/test_solidity_ast_bindings.py"
  python3 "$ROOT/scripts/test_verus_manifest.py"
  python3 "$ROOT/scripts/test_check_failure_manifests.py"
  python3 "$ROOT/scripts/test_trusted_proof_profiles.py"
  bash "$CONCRETE_RUNTIME_TEST"
  python3 "$FAILURE_MANIFEST_CHECK"
  run_proof_stage claim-manifest python3 "$CLAIM_CHECK"
  run_proof_stage lean run_lean_proofs
  run_proof_stage lean-negative run_lean_failure_fixtures
  run_proof_stage policy-vector-consumers run_policy_vector_consumers
  run_proof_stage refinement-gate run_refinement_gate
  run_proof_stage claim-transaction-tests \
    python3 "$CLAIM_TEST_CHECK"
  run_proof_stage known-answer-consumers \
    python3 "$ROOT/scripts/check_known_answer_manifest.py"
  run_proof_stage smt-and-negative run_smt
  run_proof_stage halmos-and-negative run_halmos
  run_proof_stage verus-and-negative run_verus
  python3 -c \
    'import json,sys; receipt=json.load(open(sys.argv[1])); assert receipt["complete"] is True' \
    "$PROOF_RECEIPT"
  python3 "$PROOF_IMPACT_CHECK" --receipt "$PROOF_RECEIPT"
  echo "proof_receipt=$PROOF_RECEIPT" >&2
}

require_ui_dependencies() {
  if [[ "$UI_DEPENDENCIES_READY" -eq 1 ]]; then
    return
  fi
  if [[ "${BRIDGE_TRUSTED_DEPS_READY:-0}" == "1" ]]; then
    if [[ ! -d "$ROOT/ui/node_modules" || -L "$ROOT/ui/node_modules" ]]; then
      echo "trusted UI dependencies are missing or symlinked" >&2
      return 1
    fi
  elif [[ "${CI:-}" == "true" ]]; then
    pnpm --dir "$ROOT/ui" install --frozen-lockfile
  elif [[ ! -d "$ROOT/ui/node_modules" ]]; then
    echo "ui/node_modules is missing; run pnpm --dir ui install --frozen-lockfile before checks" >&2
    return 1
  fi
  UI_DEPENDENCIES_READY=1
}

run_ui_fast() {
  require_ui_dependencies
  pnpm --dir "$ROOT/ui" run codegen:abi:check
  pnpm --dir "$ROOT/ui" run codegen:candid:check
  pnpm --dir "$ROOT/ui" run typecheck
  pnpm --dir "$ROOT/ui" run lint
  pnpm --dir "$ROOT/ui" run test
  pnpm --dir "$ROOT/ui" run build
}

run_ui_e2e() {
  require_ui_dependencies
  pnpm --dir "$ROOT/ui" run e2e
}

run_ui() {
  run_ui_fast
  run_ui_e2e
}

run_icp_build() {
  icp project show --project-root-override "$ROOT" >/dev/null
  icp build bridge-canister --project-root-override "$ROOT"
}

wait_for_anvil() {
  local _attempt
  for _attempt in {1..50}; do
    if [[ -z "$ANVIL_PID" ]] || ! kill -0 "$ANVIL_PID" 2>/dev/null; then
      echo "spawned Anvil exited before becoming ready" >&2
      return 1
    fi
    if cast chain-id --rpc-url http://127.0.0.1:8545 >/dev/null 2>&1; then
      kill -0 "$ANVIL_PID" 2>/dev/null || {
        echo "Anvil RPC is not owned by the spawned process" >&2
        return 1
      }
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

json_tuple_field() {
  local json="$1"
  local index="$2"

  python3 -c 'import json, sys; print(json.load(sys.stdin)[0][int(sys.argv[1])])' "$index" <<<"$json"
}

prepare_temporary_icp_config() {
  cp -p "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.original"
  ICP_CONFIG_BACKED_UP=1
  python3 "$ROOT/scripts/prepare_local_network.py" --project-root "$ROOT" --write >/dev/null
  cp -p "$ROOT/icp.yaml" "$TMP_ROOT/icp.yaml.applied"
}

ensure_icp_network() {
  local network_status="$1"
  local marker="$ROOT/.icp/cache/networks/local/bridge-ci-project-marker.json"

  if icp network status --json --project-root-override "$ROOT" \
    >"$network_status" 2>/dev/null; then
    python3 - "$marker" "$ROOT" <<'PY'
import json, os, sys
marker, expected_root = sys.argv[1:]
try:
    value = json.load(open(marker, encoding="utf-8"))
except (OSError, ValueError) as error:
    raise SystemExit(f"running ICP network has no valid Bridge CI project marker: {error}")
if value != {"project_root": os.path.realpath(expected_root), "network": "local", "purpose": "bridge-ci-smoke"}:
    raise SystemExit("running ICP network marker does not match this Bridge CI project")
PY
    return
  fi

  prepare_temporary_icp_config
  # Mark ownership before start so cleanup also handles signals during startup.
  ICP_NETWORK_OWNED=1
  icp network start -d --project-root-override "$ROOT"
  icp network status --json --project-root-override "$ROOT" >"$network_status"
  mkdir -p "$(dirname "$marker")"
  python3 - "$marker" "$ROOT" <<'PY'
import json, os, sys
marker, root = sys.argv[1:]
with open(marker, "w", encoding="utf-8") as output:
    json.dump({"project_root": os.path.realpath(root), "network": "local", "purpose": "bridge-ci-smoke"}, output, sort_keys=True)
    output.write("\n")
PY
}

prepare_smoke_canister_state() {
  local status="$TMP_ROOT/original-bridge-canister-status.json"
  ICP_SMOKE_STATE_PREPARED=1
  if [[ -f "$ROOT/.icp/cache/mappings/local.ids.json" ]]; then
    cp -p "$ROOT/.icp/cache/mappings/local.ids.json" "$TMP_ROOT/local.ids.json.original"
    ICP_LOCAL_MAPPING_BACKED_UP=1
  fi
  if icp canister status bridge-canister -e local --json --project-root-override "$ROOT" >"$status" 2>/dev/null; then
    ICP_TEST_CANISTER_SNAPSHOT="$(icp canister snapshot create bridge-canister -e local -q --project-root-override "$ROOT")"
    ICP_TEST_CANISTER_INSTALL_MODE="reinstall"
    if rg -qi '"status"[[:space:]]*:[[:space:]]*"running"' "$status"; then
      ICP_TEST_CANISTER_WAS_RUNNING=1
    fi
  else
    icp canister create bridge-canister -e local --project-root-override "$ROOT"
    ICP_TEST_CANISTER_CREATED=1
  fi
}

restore_smoke_canister_state() {
  local restore_failed=0 restored=0
  if [[ "$ICP_SMOKE_STATE_PREPARED" -eq 0 ]]; then
    return 0
  fi
  if [[ "$ICP_TEST_CANISTER_CREATED" -eq 1 ]]; then
    icp canister stop bridge-canister -e local --project-root-override "$ROOT" >/dev/null 2>&1 || true
    if icp canister delete bridge-canister -e local --project-root-override "$ROOT" >/dev/null 2>&1; then
      ICP_TEST_CANISTER_CREATED=0
    else
      echo "failed to delete the temporary Bridge smoke canister" >&2
      restore_failed=1
    fi
  elif [[ -n "$ICP_TEST_CANISTER_SNAPSHOT" ]]; then
    icp canister stop bridge-canister -e local --project-root-override "$ROOT" >/dev/null 2>&1 || true
    if icp canister snapshot restore bridge-canister "$ICP_TEST_CANISTER_SNAPSHOT" -e local --project-root-override "$ROOT" >/dev/null; then
      restored=1
      if icp canister snapshot delete bridge-canister "$ICP_TEST_CANISTER_SNAPSHOT" -e local --project-root-override "$ROOT" >/dev/null; then
        ICP_TEST_CANISTER_SNAPSHOT=""
      else
        echo "failed to delete the restored Bridge smoke snapshot" >&2
        restore_failed=1
      fi
    else
      echo "failed to restore the Bridge smoke snapshot; snapshot retained" >&2
      restore_failed=1
    fi
    if [[ "$ICP_TEST_CANISTER_WAS_RUNNING" -eq 1 && "$restored" -eq 1 ]]; then
      if ! icp canister start bridge-canister -e local --project-root-override "$ROOT" >/dev/null; then
        echo "failed to restart the Bridge smoke canister after cleanup" >&2
        restore_failed=1
      fi
    fi
  fi
  if [[ "$ICP_LOCAL_MAPPING_BACKED_UP" -eq 1 ]]; then
    if cp -p "$TMP_ROOT/local.ids.json.original" "$ROOT/.icp/cache/mappings/local.ids.json"; then
      ICP_LOCAL_MAPPING_BACKED_UP=0
    else
      echo "failed to restore the local canister mapping" >&2
      restore_failed=1
    fi
  elif [[ "$ICP_TEST_CANISTER_CREATED" -eq 0 ]]; then
    rm -f "$ROOT/.icp/cache/mappings/local.ids.json" || restore_failed=1
  fi
  if [[ "$restore_failed" -eq 0 ]]; then
    ICP_SMOKE_STATE_PREPARED=0
  elif [[ "$restored" -eq 0 && -n "$ICP_TEST_CANISTER_SNAPSHOT" ]]; then
    echo "snapshot $ICP_TEST_CANISTER_SNAPSHOT remains available for manual recovery" >&2
  fi
  return "$restore_failed"
}

run_smoke() {
  local -a bridge_fee_constructor_args
  local network_status="$TMP_ROOT/icp-network-status.json"
  local canister_status="$TMP_ROOT/canister-status.json"
  local bridge_status
  local bridge_address
  local base_admin_timelock
  local bsns_address
  local token_bridge
  local token_name
  local token_symbol
  local token_version
  local token_decimals
  local recipient_balance
  local token_total_supply
  local minted_in_window
  local processed
  local release_withdrawal
  local next_withdrawal_id
  local timelock_delay
  local proposer_role
  local canceller_role
  local executor_role
  local default_admin_role
  local unpause_deposit_data
  local rotate_signer_data
  local management_salt
  local current_service_fee
  local limit_caller
  local limit_signature
  local smoke_principal
  local bridge_init_args
  local bridge_signer="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
  local -r rotated_bridge_signer="0x976EA74026E726554dB657fA54763abd0C3a0aa9"
  local -r second_rotated_bridge_signer="0x14dC79964da2C08b23698B3D3cc7Ca32193d9955"
  local -r runtime_administrator="0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC"
  local -r unauthorized_wallet="0x90F79bf6EB2c4f870365E785982E1f101E93b906"
  local -r independent_canceller="0x9965507D1a55bcC2695C58ba16FB37d819B0A4dc"
  local -r recipient="0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65"
  local -r deposit_id="0x0000000000000000000000000000000000000000000000000000000000000001"
  local -r gross_amount="101000000"
  local -r service_fee="1000000"
  bridge_fee_constructor_args=("1" "100000000" "$service_fee")
  local -r minted_amount="100000000"
  local -r release_amount="50000000"
  local -r release_amount_out="49000000"
  local -r principal_owner="0x010203"
  local -r default_subaccount="0x0000000000000000000000000000000000000000000000000000000000000000"
  local -r timelock_delay_seconds="86400"
  local -r zero_address="0x0000000000000000000000000000000000000000"
  local -r zero_bytes32="0x0000000000000000000000000000000000000000000000000000000000000000"

  ensure_icp_network "$network_status"
  prepare_smoke_canister_state
  smoke_principal="$(icp identity principal --project-root-override "$ROOT")"
  bridge_init_args="(record {
    ledger_canister_id = principal \"73mez-iiaaa-aaaaq-aaasq-cai\";
    index_canister_id = principal \"7vojr-tyaaa-aaaaq-aaatq-cai\";
    evm_rpc_canister_id = principal \"73mez-iiaaa-aaaaq-aaasq-cai\";
    custom_evm_rpc_urls = vec {};
    base_chain_id = 8_453 : nat64;
    bridge_contract = blob \"\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\\01\";
    expected_bridge_runtime_sha256 = blob \"\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\\04\";
    timelock_contract = blob \"\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\\02\";
    expected_timelock_minimum_delay_seconds = 86_400 : nat64;
    expected_bsns_runtime_sha256 = blob \"\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\\05\";
    expected_bsns_decimals = 8 : nat8;
    expected_minimum_service_fee = 10_000 : nat;
    deployment_instance_id = blob \"\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\\03\";
    minimum_withdrawal_id = blob \"\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\00\\01\";
    ecdsa_key_name = \"dfx_test_key\";
    ecdsa_derivation_path = vec {};
    governance_ecdsa_derivation_path = vec { blob \"governance-operator\" };
    deposit_rate_limit_window_seconds = 60 : nat64;
    deposit_rate_limit_global = 30 : nat16;
    deposit_rate_limit_per_principal = 3 : nat16;
    notification_rate_limit_window_seconds = 600 : nat64;
    notification_rate_limit_global = 60 : nat16;
    notification_ingestion_rate_limit_global = 30 : nat16;
    settlement_rate_limit_window_seconds = 600 : nat64;
    settlement_rate_limit_global = 60 : nat16;
    settlement_rate_limit_per_principal = 6 : nat16;
    settlement_rate_limit_per_record = 3 : nat16;
    settlement_retry_interval_seconds = 60 : nat64;
    governance_evm_fee = record {
      gas_limit_ceiling = 500_000 : nat;
      max_fee_per_gas_ceiling = 200_000_000_000 : nat;
      max_priority_fee_per_gas_ceiling = 10_000_000_000 : nat;
      l1_fee_per_transaction_ceiling_wei = 10_000_000_000_000_000 : nat;
      quote_validity_seconds = 90 : nat64;
      gas_limit_multiplier_bps = 13_000 : nat32;
      base_fee_multiplier_bps = 60_000 : nat32;
      l1_fee_multiplier_bps = 15_000 : nat32;
    };
    governance_replacement = record {
      max_replacements = 3 : nat8;
      fee_bump_bps = 1_250 : nat16;
    };
    cycles_floor = 1 : nat;
    settlement_cycle_ceiling = 1 : nat;
    governance_principal = principal \"$smoke_principal\";
    pause_principal = principal \"7jkta-eyaaa-aaaaq-aaarq-cai\";
    confirmation_relayer_principal = principal \"rrkah-fqaaa-aaaaa-aaaaq-cai\";
    fee_recipient = record {
      owner = principal \"aaaaa-aa\";
      subaccount = blob \"\";
    };
  })"
  # The local smoke must install the explicitly built test-deployment artifact. Using
  # `icp deploy` here would rebuild the production package from the recipe and its
  # fail-closed mainnet configuration guard would (correctly) reject these local IDs.
  icp canister install bridge-canister \
    -e local \
    --mode "$ICP_TEST_CANISTER_INSTALL_MODE" \
    --yes \
    --wasm "$ROOT/target/test-deployment/staging/bridge_canister.wasm" \
    --args "$bridge_init_args" \
    --project-root-override "$ROOT"
  icp canister status bridge-canister -e local --json \
    --project-root-override "$ROOT" >"$canister_status"
  if ! rg -qi '"status"[[:space:]]*:[[:space:]]*"running"' "$canister_status"; then
    echo "bridge-canister is not running" >&2
    cat "$canister_status" >&2
    return 1
  fi
  if icp canister call bridge-canister get_runtime_binding '()' \
    -e local --query --json \
    --candid "$ROOT/canister/bridge-canister/bridge.did" \
    --project-root-override "$ROOT" >/dev/null 2>&1; then
    echo "get_runtime_binding succeeded before chain-key address initialization" >&2
    return 1
  fi
  public_config_initialization="$(
    icp canister call bridge-canister initialize_public_config '()' \
      -e local --json \
      --candid "$ROOT/canister/bridge-canister/bridge.did" \
      --project-root-override "$ROOT"
  )"
  python3 -c '
import json, re, sys
response = json.loads(sys.stdin.read())
candid = response.get("response_candid") or ""
if not re.search(r"variant\s*\{\s*Ok\s*\}", candid):
    raise SystemExit(f"public configuration initialization failed: {response!r}")
' <<<"$public_config_initialization"
  icp canister call bridge-canister get_runtime_binding '()' \
    -e local --query --json \
    --candid "$ROOT/canister/bridge-canister/bridge.did" \
    --project-root-override "$ROOT" >/dev/null
  bridge_status="$(
    icp canister call bridge-canister get_bridge_status '()' \
      -e local --query --json \
      --candid "$ROOT/canister/bridge-canister/bridge.did" \
      --project-root-override "$ROOT"
  )"
  expected_schema_version="$(python3 "$ROOT/scripts/check_schema_consistency.py" --print-version)"
  python3 -c '
import json, re, sys

response = json.load(sys.stdin)
candid = response.get("response_candid") or ""
expected = {
    "schema_version": (sys.argv[1], "nat16"),
    "deposits": ("0", "nat64"),
    "withdrawals": ("0", "nat64"),
    "reconciliation_holds": ("0", "nat64"),
}
for field, (value, candid_type) in expected.items():
    pattern = rf"\b{field}\s*=\s*{value}\s*:\s*{candid_type}\b"
    if len(re.findall(pattern, candid)) != 1:
        raise SystemExit(f"unexpected get_bridge_status response: {response!r}")
' "$expected_schema_version" <<<"$bridge_status"

  icp canister install bridge-canister \
    -e local \
    --mode upgrade \
    --wasm "$ROOT/target/test-deployment/staging/bridge_canister.wasm" \
    --project-root-override "$ROOT"
  icp canister call bridge-canister get_runtime_binding '()' \
    -e local --query --json \
    --candid "$ROOT/canister/bridge-canister/bridge.did" \
    --project-root-override "$ROOT" >/dev/null
  bridge_status_after_upgrade="$(
    icp canister call bridge-canister get_bridge_status '()' \
      -e local --query --json \
      --candid "$ROOT/canister/bridge-canister/bridge.did" \
      --project-root-override "$ROOT"
  )"
  python3 -c '
import json, re, sys

before = json.loads(sys.argv[1]).get("response_candid") or ""
after = json.loads(sys.argv[2]).get("response_candid") or ""
stable_fields = {
    "schema_version": (sys.argv[3], "nat16"),
    "deposits": ("0", "nat64"),
    "withdrawals": ("0", "nat64"),
    "reconciliation_holds": ("0", "nat64"),
    "deposits_paused": ("true", "bool"),
}
for field, (value, candid_type) in stable_fields.items():
    pattern = rf"\b{field}\s*=\s*{value}(?:\s*:\s*{candid_type})?\b"
    if len(re.findall(pattern, before)) != 1 or len(re.findall(pattern, after)) != 1:
        raise SystemExit(f"same-Wasm upgrade did not preserve empty state field {field}")
' "$bridge_status" "$bridge_status_after_upgrade" "$expected_schema_version"

  if cast chain-id --rpc-url http://127.0.0.1:8545 >/dev/null 2>&1; then
    echo "port 8545 is already serving an EVM node; refusing to reuse it" >&2
    return 1
  fi
  anvil --chain-id 31337 --host 127.0.0.1 --port 8545 --silent >"$TMP_ROOT/anvil.log" 2>&1 &
  ANVIL_PID=$!
  wait_for_anvil
  if [[ "$(cast chain-id --rpc-url http://127.0.0.1:8545)" != "31337" ]]; then
    echo "unexpected Anvil chain ID" >&2
    return 1
  fi

  base_admin_timelock="$(deploy_contract \
    "src/BridgeTimelockController.sol:BridgeTimelockController" \
    "$timelock_delay_seconds" \
    "[$runtime_administrator]" \
    "[$independent_canceller]" \
    "[$runtime_administrator]")"
  read -r timelock_delay _ <<<"$(
    cast call "$base_admin_timelock" "getMinDelay()(uint256)" --rpc-url http://127.0.0.1:8545
  )"
  require_equal "Base Admin timelock delay" "$timelock_delay" "$timelock_delay_seconds"
  proposer_role="$(
    cast call "$base_admin_timelock" "PROPOSER_ROLE()(bytes32)" --rpc-url http://127.0.0.1:8545
  )"
  canceller_role="$(
    cast call "$base_admin_timelock" "CANCELLER_ROLE()(bytes32)" --rpc-url http://127.0.0.1:8545
  )"
  executor_role="$(
    cast call "$base_admin_timelock" "EXECUTOR_ROLE()(bytes32)" --rpc-url http://127.0.0.1:8545
  )"
  default_admin_role="$(
    cast call "$base_admin_timelock" "DEFAULT_ADMIN_ROLE()(bytes32)" --rpc-url http://127.0.0.1:8545
  )"
  require_equal \
    "Governance Operator proposer role" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$proposer_role" "$runtime_administrator" \
      --rpc-url http://127.0.0.1:8545)" \
    "true"
  require_equal \
    "Governance Operator has no canceller role" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$canceller_role" "$runtime_administrator" \
      --rpc-url http://127.0.0.1:8545)" \
    "false"
  require_equal \
    "independent canceller role" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$canceller_role" "$independent_canceller" \
      --rpc-url http://127.0.0.1:8545)" \
    "true"
  require_equal \
    "independent canceller has no proposer role" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$proposer_role" "$independent_canceller" \
      --rpc-url http://127.0.0.1:8545)" \
    "false"
  require_equal \
    "independent canceller has no executor role" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$executor_role" "$independent_canceller" \
      --rpc-url http://127.0.0.1:8545)" \
    "false"
  require_equal \
    "Governance Operator executor role" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$executor_role" "$runtime_administrator" \
      --rpc-url http://127.0.0.1:8545)" \
    "true"
  require_equal \
    "permissionless executor disabled" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$executor_role" "$zero_address" \
      --rpc-url http://127.0.0.1:8545)" \
    "false"
  require_equal \
    "Timelock self administration" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$default_admin_role" \
      "$base_admin_timelock" --rpc-url http://127.0.0.1:8545)" \
    "true"
  require_equal \
    "Governance Operator has no direct timelock administration" \
    "$(cast call "$base_admin_timelock" "hasRole(bytes32,address)(bool)" "$default_admin_role" "$runtime_administrator" \
      --rpc-url http://127.0.0.1:8545)" \
    "false"

  bridge_address="$(deploy_contract \
    "src/Bridge.sol:Bridge" \
    "$bridge_signer" \
    "$runtime_administrator" \
    "$base_admin_timelock" \
    "$(cast codehash "$base_admin_timelock" --rpc-url http://127.0.0.1:8545)" \
    "1000000000000" \
    "10000000000000" \
    "3600" \
    "${bridge_fee_constructor_args[@]}")"
  for limit_caller in "$bridge_signer" "$runtime_administrator" "$unauthorized_wallet"; do
    for limit_signature in \
      "setMintLimits(uint256,uint256,uint64)" \
      "reduceMintLimits(uint256,uint256,uint64)"; do
      if cast call \
        "$bridge_address" \
        "$limit_signature" \
        1 \
        1 \
        1 \
        --rpc-url http://127.0.0.1:8545 \
        --from "$limit_caller" >/dev/null 2>&1; then
        echo "removed Mint limit selector remains callable: $limit_signature" >&2
        return 1
      fi
    done
  done
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
  require_equal "bSNS name" "$token_name" '"KINIC"'
  require_equal "bSNS symbol" "$token_symbol" '"KINIC"'
  require_equal "bSNS EIP-712 version" "$token_version" '"1"'
  require_equal "bSNS decimals" "$token_decimals" "8"

  require_equal \
    "Initial Deposit mint pause" \
    "$(cast call "$bridge_address" "depositMintsPaused()(bool)" --rpc-url http://127.0.0.1:8545)" \
    "true"
  require_equal \
    "Initial Withdrawal pause" \
    "$(cast call "$bridge_address" "withdrawalsPaused()(bool)" --rpc-url http://127.0.0.1:8545)" \
    "true"
  # Local smoke activation uses Anvil impersonation only to reach the asset-flow checks without
  # waiting 24 hours. The same run separately verifies that the real admin wallet needs Timelock.
  cast rpc anvil_impersonateAccount "$base_admin_timelock" --rpc-url http://127.0.0.1:8545 >/dev/null
  cast rpc anvil_setBalance "$base_admin_timelock" 0x56BC75E2D63100000 --rpc-url http://127.0.0.1:8545 >/dev/null
  cast send "$bridge_address" "rotateBridgeSigner(address)" "$rotated_bridge_signer" \
    --rpc-url http://127.0.0.1:8545 --from "$base_admin_timelock" --unlocked >/dev/null
  bridge_signer="$rotated_bridge_signer"
  cast send "$bridge_address" "unpauseDepositMints()" \
    --rpc-url http://127.0.0.1:8545 --from "$base_admin_timelock" --unlocked >/dev/null
  cast send "$bridge_address" "unpauseWithdrawals()" \
    --rpc-url http://127.0.0.1:8545 --from "$base_admin_timelock" --unlocked >/dev/null
  cast rpc anvil_stopImpersonatingAccount "$base_admin_timelock" --rpc-url http://127.0.0.1:8545 >/dev/null

  local authorization_epoch deadline typed_data signature
  authorization_epoch="$(
    cast call "$bridge_address" "mintAuthorizationEpoch()(uint256)" \
      --rpc-url http://127.0.0.1:8545
  )"
  deadline="$(( $(cast block latest --rpc-url http://127.0.0.1:8545 --field timestamp) + 600 ))"
  typed_data="$(
    jq -cn \
      --arg deposit_id "$deposit_id" \
      --arg recipient "$recipient" \
      --arg bridge "$bridge_address" \
      --argjson gross_amount "$gross_amount" \
      --argjson max_service_fee "$service_fee" \
      --argjson charged_service_fee "$service_fee" \
      --argjson deadline "$deadline" \
      --argjson authorization_epoch "$authorization_epoch" \
      '{
        types: {
          EIP712Domain: [
            {name:"name",type:"string"},
            {name:"version",type:"string"},
            {name:"chainId",type:"uint256"},
            {name:"verifyingContract",type:"address"}
          ],
          MintAuthorization: [
            {name:"depositId",type:"bytes32"},
            {name:"recipient",type:"address"},
            {name:"grossAmount",type:"uint256"},
            {name:"maxServiceFee",type:"uint256"},
            {name:"chargedServiceFee",type:"uint256"},
            {name:"deadline",type:"uint256"},
            {name:"authorizationEpoch",type:"uint256"}
          ]
        },
        primaryType:"MintAuthorization",
        domain:{name:"KINIC Bridge",version:"1",chainId:31337,verifyingContract:$bridge},
        message:{
          depositId:$deposit_id,
          recipient:$recipient,
          grossAmount:$gross_amount,
          maxServiceFee:$max_service_fee,
          chargedServiceFee:$charged_service_fee,
          deadline:$deadline,
          authorizationEpoch:$authorization_epoch
        }
      }'
  )"
  signature="$(
    cast rpc eth_signTypedData_v4 "$bridge_signer" "$typed_data" \
      --rpc-url http://127.0.0.1:8545 | tr -d '"'
  )"
  cast send \
    "$bridge_address" \
    "mintDepositWithAuthorization((bytes32,address,uint256,uint256,uint256,uint256,uint256),bytes)" \
    "($deposit_id,$recipient,$gross_amount,$service_fee,$service_fee,$deadline,$authorization_epoch)" \
    "$signature" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$recipient" \
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

  cast send \
    "$bsns_address" \
    "approve(address,uint256)" \
    "$bridge_address" \
    "$release_amount" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$recipient" \
    --unlocked >/dev/null
  cast send \
    "$bridge_address" \
    "createWithdrawal(uint256,uint256,bytes,bytes32)" \
    "$release_amount" \
    "$service_fee" \
    "$principal_owner" \
    "$default_subaccount" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$recipient" \
    --unlocked >/dev/null
  release_withdrawal="$(
    cast call \
      "$bridge_address" \
      "getWithdrawal(uint256)((address,uint256,uint256,uint256,uint256,bytes,bytes32,uint8))" \
      1 \
      --rpc-url http://127.0.0.1:8545 \
      --json
  )"
  require_equal \
    "Committed requester" \
    "$(printf '%s' "$(json_tuple_field "$release_withdrawal" 0)" | tr '[:upper:]' '[:lower:]')" \
    "$(printf '%s' "$recipient" | tr '[:upper:]' '[:lower:]')"
  require_equal "Committed amount" "$(json_tuple_field "$release_withdrawal" 1)" "$release_amount"
  require_equal "Committed max Service Fee" "$(json_tuple_field "$release_withdrawal" 2)" "$service_fee"
  require_equal "Committed charged Service Fee" "$(json_tuple_field "$release_withdrawal" 3)" "$service_fee"
  require_equal "Committed amount out" "$(json_tuple_field "$release_withdrawal" 4)" "$release_amount_out"
  require_equal "Committed owner" "$(json_tuple_field "$release_withdrawal" 5)" "$principal_owner"
  require_equal "Committed subaccount" "$(json_tuple_field "$release_withdrawal" 6)" "$default_subaccount"
  require_equal "Committed status" "$(json_tuple_field "$release_withdrawal" 7)" "1"

  read -r recipient_balance _ <<<"$(
    cast call "$bsns_address" "balanceOf(address)(uint256)" "$recipient" --rpc-url http://127.0.0.1:8545
  )"
  read -r token_total_supply _ <<<"$(
    cast call "$bsns_address" "totalSupply()(uint256)" --rpc-url http://127.0.0.1:8545
  )"
  read -r minted_in_window _ <<<"$(
    cast call "$bridge_address" "mintedInWindow()(uint256)" --rpc-url http://127.0.0.1:8545
  )"
  read -r next_withdrawal_id _ <<<"$(
    cast call "$bridge_address" "nextWithdrawalId()(uint256)" --rpc-url http://127.0.0.1:8545
  )"
  require_equal "post-settlement recipient balance" "$recipient_balance" "50000000"
  require_equal "post-settlement token supply" "$token_total_supply" "50000000"
  require_equal "Withdrawal does not consume mint window" "$minted_in_window" "$minted_amount"
  require_equal "next Withdrawal ID" "$next_withdrawal_id" "2"

  cast send \
    "$bridge_address" \
    "setServiceFee(uint256)" \
    "2000000" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$runtime_administrator" \
    --unlocked >/dev/null
  read -r current_service_fee _ <<<"$(
    cast call "$bridge_address" "serviceFee()(uint256)" --rpc-url http://127.0.0.1:8545
  )"
  require_equal "Runtime Administrator Service Fee" "$current_service_fee" "2000000"
  cast send \
    "$bridge_address" \
    "pauseDepositMints()" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$runtime_administrator" \
    --unlocked >/dev/null
  cast send \
    "$bridge_address" \
    "pauseWithdrawals()" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$runtime_administrator" \
    --unlocked >/dev/null
  require_equal \
    "Deposit mint pause" \
    "$(cast call "$bridge_address" "depositMintsPaused()(bool)" --rpc-url http://127.0.0.1:8545)" \
    "true"
  require_equal \
    "Withdrawal pause" \
    "$(cast call "$bridge_address" "withdrawalsPaused()(bool)" --rpc-url http://127.0.0.1:8545)" \
    "true"

  if cast call \
    "$bridge_address" \
    "unpauseDepositMints()" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$unauthorized_wallet" >/dev/null 2>&1; then
    echo "Base Admin wallet bypassed the timelock" >&2
    return 1
  fi
  unpause_deposit_data="$(cast calldata "unpauseDepositMints()")"
  rotate_signer_data="$(cast calldata "rotateBridgeSigner(address)" "$second_rotated_bridge_signer")"
  management_salt="0x0000000000000000000000000000000000000000000000000000000000000001"
  cast send \
    "$base_admin_timelock" \
    "scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)" \
    "[$bridge_address,$bridge_address]" \
    "[0,0]" \
    "[$rotate_signer_data,$unpause_deposit_data]" \
    "$zero_bytes32" \
    "$management_salt" \
    "$timelock_delay_seconds" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$runtime_administrator" \
    --unlocked >/dev/null
  if cast send \
    "$base_admin_timelock" \
    "executeBatch(address[],uint256[],bytes[],bytes32,bytes32)" \
    "[$bridge_address,$bridge_address]" \
    "[0,0]" \
    "[$rotate_signer_data,$unpause_deposit_data]" \
    "$zero_bytes32" \
    "$management_salt" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$runtime_administrator" \
    --unlocked >/dev/null 2>&1; then
    echo "Base Admin operation executed before the 24-hour delay" >&2
    return 1
  fi
  cast rpc evm_increaseTime "$timelock_delay_seconds" --rpc-url http://127.0.0.1:8545 >/dev/null
  cast rpc evm_mine --rpc-url http://127.0.0.1:8545 >/dev/null
  cast send \
    "$base_admin_timelock" \
    "executeBatch(address[],uint256[],bytes[],bytes32,bytes32)" \
    "[$bridge_address,$bridge_address]" \
    "[0,0]" \
    "[$rotate_signer_data,$unpause_deposit_data]" \
    "$zero_bytes32" \
    "$management_salt" \
    --rpc-url http://127.0.0.1:8545 \
    --from "$runtime_administrator" \
    --unlocked >/dev/null
  require_equal \
    "Timelocked Deposit mint unpause" \
    "$(cast call "$bridge_address" "depositMintsPaused()(bool)" --rpc-url http://127.0.0.1:8545)" \
    "false"
  require_equal \
    "Independent Withdrawal pause" \
    "$(cast call "$bridge_address" "withdrawalsPaused()(bool)" --rpc-url http://127.0.0.1:8545)" \
    "true"
  echo "Bridge-created bSNS deployed at $bsns_address" >&2
}

run_real() {
  require_ui_dependencies
  if [[ "${BRIDGE_TRUSTED_DEPS_READY:-0}" == "1" ]]; then
    pnpm --dir "$ROOT/ui" exec playwright test --config playwright.real.config.ts
  else
    pnpm --dir "$ROOT/ui" e2e:real
  fi
}

run_smoke_step() {
  trap cleanup_runtime EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  run_smoke
}

case "$MODE" in
  all)
    run_step versions run_versions
    run_step rust run_rust
    run_step contracts run_contracts
    run_step proofs run_proofs
    run_step ui run_ui
    run_step icp run_icp_build
    run_step real run_real
    run_step smoke run_smoke_step
    ;;
  checks)
    run_step versions run_versions
    run_step rust run_rust
    run_step contracts run_contracts
    run_step proofs run_proofs
    run_step ui run_ui
    run_step icp run_icp_build
    ;;
  versions)
    run_step versions run_versions
    ;;
  rust)
    run_step rust run_rust
    ;;
  rust-fast)
    run_step rust-fast run_rust_fast
    ;;
  rust-integration)
    run_step rust-integration run_rust_integration
    ;;
  contracts)
    run_step contracts run_contracts
    ;;
  contracts-fast)
    run_step contracts-fast run_contracts_fast
    ;;
  contracts-coverage)
    run_step contracts-coverage run_contracts_coverage
    ;;
  certora)
    run_step certora run_certora_advisory_checks
    ;;
  proofs)
    run_step versions run_versions
    run_step proofs run_proofs
    ;;
  ui)
    run_step versions run_versions
    run_step ui run_ui
    ;;
  ui-fast)
    run_step ui-fast run_ui_fast
    ;;
  ui-e2e)
    run_step ui-e2e run_ui_e2e
    ;;
  icp)
    run_step icp run_icp_build
    ;;
  smoke)
    run_step smoke run_smoke_step
    ;;
  real)
    run_step real run_real
    ;;
  *)
    echo "usage: $0 {all|checks|versions|rust|rust-fast|rust-integration|contracts|contracts-fast|contracts-coverage|proofs|ui|ui-fast|ui-e2e|icp|smoke|real}" >&2
    exit 2
    ;;
esac
