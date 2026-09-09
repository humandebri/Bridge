#!/usr/bin/env python3
"""Static regression tests for local CI mode composition."""

from pathlib import Path
import re
import shlex
import subprocess
import tempfile
import unittest


SOURCE = (Path(__file__).parent / "ci-local.sh").read_text(encoding="utf-8")
GUARDS = (Path(__file__).parent / "ci_guards.sh").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    match = re.search(rf"^{re.escape(name)}\(\) \{{\n(?P<body>.*?)^\}}$", SOURCE, re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError(f"missing function: {name}")
    return match.group("body")


def mode_body(name: str) -> str:
    match = re.search(
        rf"^  {re.escape(name)}\)\n(?P<body>.*?)^    ;;$",
        SOURCE,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"missing mode: {name}")
    return match.group("body")


class CiModeTests(unittest.TestCase):
    def test_local_driver_selects_the_pinned_node_with_fnm_once(self) -> None:
        self.assertIn('EXPECTED_NODE_VERSION="v$(<"$ROOT/.node-version")"', SOURCE)
        self.assertIn('BRIDGE_CI_LOCAL_NODE_REEXEC:-0', SOURCE)
        self.assertIn('exec fnm exec --using "$EXPECTED_NODE_VERSION" env \\', SOURCE)
        self.assertIn('BRIDGE_CI_LOCAL_NODE_REEXEC=1 "$ROOT/scripts/ci-local.sh"', SOURCE)

    def assert_calls(self, aggregate: str, expected: list[str]) -> None:
        body = function_body(aggregate)
        positions = [body.find(f"  {name}\n") for name in expected]
        self.assertTrue(all(position >= 0 for position in positions), (aggregate, positions))
        self.assertEqual(positions, sorted(positions))

    def test_all_builds_test_deployment_wasm_before_smoke(self) -> None:
        body = mode_body("all")
        rust = body.index("    run_step rust run_rust\n")
        real = body.index("    run_step real run_real\n")
        smoke = body.index("    run_step smoke run_smoke_step\n")
        self.assertLess(rust, smoke)
        self.assertLess(rust, real)
        self.assertLess(real, smoke)
        self.assertTrue(body.rstrip().endswith("run_step smoke run_smoke_step"))

    def test_smoke_bridge_deploy_uses_profile_specific_constructor_shape(self) -> None:
        start = SOURCE.index('bridge_address="$(deploy_contract \\\n    "src/Bridge.sol:Bridge"')
        terminator = '    "${bridge_fee_constructor_args[@]}")"'
        end = SOURCE.index(terminator, start) + len(terminator)
        deployment = SOURCE[start:end]
        self.assertNotIn('"kinic"', deployment)
        self.assertNotIn('"KINIC"', deployment)
        self.assertIn(
            'bridge_fee_constructor_args=("1" "100000000" "$service_fee")', SOURCE
        )
        self.assertNotIn('bridge_fee_constructor_args=("100000000" "$service_fee")', SOURCE)
        self.assertIn('require_equal "bSNS name" "$token_name" \'"KINIC"\'', SOURCE)

    def test_legacy_aggregate_modes_remain_complete(self) -> None:
        self.assert_calls("run_rust", ["run_rust_fast", "run_rust_integration"])
        self.assert_calls("run_contracts", ["run_contracts_fast", "run_contracts_coverage"])
        self.assert_calls("run_ui", ["run_ui_fast", "run_ui_e2e"])

    def test_contract_coverage_uses_thresholded_lcov(self) -> None:
        body = function_body("run_contracts_coverage")
        self.assertIn("--report lcov", body)
        self.assertIn('--report-file "$coverage_file"', body)
        self.assertIn('check_contract_coverage.py" "$coverage_file"', body)
        self.assertNotIn("--report summary", body)
        self.assertIn("BridgeTimelockController", body)

    def test_impacted_proofs_run_only_selected_stage_commands_without_receipt(self) -> None:
        body = function_body("run_impacted_proofs")
        self.assertIn('--paths-json "$changed_paths_json"', body)
        self.assertIn('run_proof_stage_command "$stage"', body)
        self.assertIn("no formal proof receipt was generated", body)
        self.assertNotIn("run_proof_stage ", body)
        self.assertNotIn("PROOF_RECEIPT", body)

        mode = mode_body("proofs-impacted")
        self.assertIn("run_step proof-preflight run_proof_preflight", mode)
        self.assertIn('run_step proofs-impacted run_impacted_proofs "$2"', mode)
        self.assertNotIn("run_step versions", mode)
        self.assertNotIn("run_step proofs run_proofs", mode)

    def test_impacted_proof_execution_does_not_write_a_formal_receipt(self) -> None:
        body = function_body("run_impacted_proofs")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            impact = root / "impact.py"
            claim_tests = root / "claim-tests.py"
            paths = root / "paths.json"
            stages = root / "stages.txt"
            receipt = root / "proof-receipt.json"
            impact.write_text(
                'print(\'{"stages":["claim-manifest","claim-transaction-tests"]}\')\n',
                encoding="utf-8",
            )
            claim_tests.write_text(
                'import sys\nprint("claim-test " + " ".join(sys.argv[1:]))\n',
                encoding="utf-8",
            )
            paths.write_text('[]\n', encoding="utf-8")
            receipt.write_text("formal receipt sentinel\n", encoding="utf-8")
            script = f"""
set -euo pipefail
run_impacted_proofs() {{
{body}
}}
initialize_proof_context() {{
  PROOF_IMPACT_CHECK={shlex.quote(str(impact))}
  CLAIM_TEST_CHECK={shlex.quote(str(claim_tests))}
}}
run_proof_stage_command() {{ printf '%s\\n' "$1" >>{shlex.quote(str(stages))}; }}
TMP_ROOT={shlex.quote(str(root))}
run_impacted_proofs {shlex.quote(str(paths))}
test "$(cat {shlex.quote(str(receipt))})" = "formal receipt sentinel"
test "$(cat {shlex.quote(str(stages))})" = 'claim-manifest'
"""
            result = subprocess.run(
                ["bash", "-c", script], capture_output=True, text=True
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                [line for line in result.stdout.splitlines() if line.startswith("claim-test ")],
                [
                    f"claim-test --impact-json {root / 'proof-impact.json'} --plan-only",
                    f"claim-test --impact-json {root / 'proof-impact.json'}",
                ],
            )

    def test_full_proofs_still_emit_complete_ten_stage_receipt(self) -> None:
        body = function_body("run_proofs")
        for stage in (
            "claim-manifest",
            "lean",
            "lean-negative",
            "policy-vector-consumers",
            "refinement-gate",
            "claim-transaction-tests",
            "known-answer-consumers",
            "smt-and-negative",
            "halmos-and-negative",
            "verus-and-negative",
        ):
            self.assertIn(f"run_proof_stage {stage}", body)
        self.assertIn('receipt["complete"] is True', body)

    def test_proofs_use_independent_claim_stages(self) -> None:
        body = function_body("run_proofs")
        receipt_regression = body.index('python3 "$ROOT/scripts/test_write_proof_receipt.py"')
        claim_manifest = body.index(
            'run_proof_stage claim-manifest python3 "$CLAIM_CHECK"'
        )
        self.assertLess(receipt_regression, claim_manifest)
        self.assertIn('python3 "$CLAIM_TEST_TEST"', body)
        self.assertIn('python3 "$ROOT/scripts/test_check_claim_manifest.py"', body)
        self.assertIn("run_proof_stage claim-transaction-tests", body)

    def test_proof_preflight_validates_claim_links_without_running_tests(self) -> None:
        body = function_body("run_proof_preflight")
        expected = [
            'python3 "$ROOT/scripts/check_schema_consistency.py"',
            'python3 "$ROOT/scripts/check_proof_impact.py"',
            'python3 "$ROOT/scripts/check_claim_manifest.py"',
            'python3 "$ROOT/scripts/check_claim_test_manifest.py" --validate-only',
        ]
        positions = [body.index(command) for command in expected]
        self.assertEqual(positions, sorted(positions))

        for mode in ("all", "checks", "proofs"):
            mode_source = mode_body(mode)
            preflight = mode_source.index("run_step proof-preflight run_proof_preflight")
            versions = mode_source.index("run_step versions run_versions")
            proofs = mode_source.index("run_step proofs run_proofs")
            self.assertLess(preflight, versions)
            self.assertLess(versions, proofs)

    def test_certora_preflight_precedes_proof_impact_validation(self) -> None:
        body = function_body("run_versions")
        ast_build = body.index('forge build --root "$ROOT/contracts" --ast')
        certora_manifest = body.index(
            'python3 "$ROOT/scripts/check_certora_manifest.py"'
        )
        certora_tests = body.index('python3 "$ROOT/scripts/test_certora_manifest.py"')
        proof_impact = body.index('python3 "$ROOT/scripts/test_proof_impact.py"')
        self.assertLess(ast_build, certora_manifest)
        self.assertLess(certora_manifest, certora_tests)
        self.assertLess(certora_tests, proof_impact)

    def test_shared_verus_kernels_may_be_private_const_or_non_const(self) -> None:
        body = function_body("run_verus")
        self.assertIn('^(pub )?(const )?fn ${kernel_name}\\b', body)

    def test_verus_positive_checks_propagate_failures(self) -> None:
        body = function_body("run_verus")
        self.assertIn(
            'verus --no-cheating "$ROOT/verification/verus/pass.rs" '
            '-o "$TMP_ROOT/verus-pass" || return',
            body,
        )
        self.assertIn(
            'python3 "$ROOT/scripts/check_verus_manifest.py" || return',
            body,
        )

    def test_halmos_runs_each_manifest_obligation_individually(self) -> None:
        body = function_body("run_halmos")
        self.assertIn(
            "read -r row_type obligation_id _strength positive_link",
            body,
        )
        self.assertIn('--function "$positive_function"', body)
        self.assertIn("Symbolic test result: 1 passed; 0 failed", body)
        self.assertNotIn("Symbolic test result: 3 passed; 0 failed", body)

    def test_smt_checks_trusted_sources_before_building(self) -> None:
        body = function_body("run_smt")
        source_check = body.index('smt_obligations.py" --check-sources')
        build = body.index("forge build")
        self.assertLess(source_check, build)

    def test_smoke_uses_profile_specific_bridge_constructor_arguments(self) -> None:
        self.assertIn(
            'bridge_fee_constructor_args=("1" "100000000" "$service_fee")', SOURCE
        )
        self.assertNotIn('bridge_fee_constructor_args=("100000000"', SOURCE)
        self.assertIn('"${bridge_fee_constructor_args[@]}"', SOURCE)

    def test_proof_stage_stops_on_the_first_failed_command(self) -> None:
        body = function_body("run_proof_stage")
        marker = '"$PROOF_FINGERPRINT" --check "$PROOF_SOURCE_BASELINE"'
        before = body.index(marker)
        command = body.index('    "$@"')
        after = body.index(marker, before + 1)
        pass_record = body.index("    stage_status=pass")
        self.assertLess(before, command)
        self.assertLess(command, after)
        self.assertLess(after, pass_record)
        self.assertNotIn('if ! "$@"', body)
        refinement = function_body("run_refinement_gate")
        commands = [line.strip() for line in refinement.splitlines() if line.strip()]
        self.assertTrue(all(command.endswith("|| return") for command in commands[:-1]))

    def test_proof_stage_function_failure_cannot_be_overwritten(self) -> None:
        body = function_body("run_proof_stage")
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            baseline = root / "baseline.json"
            stages = root / "stages.tsv"
            receipt = root / "receipt.json"
            baseline.write_text("{}\n", encoding="utf-8")
            script = f"""
set -uo pipefail
run_proof_stage() {{
{body}
}}
python3() {{ return 0; }}
fail_then_succeed() {{ false; :; }}
PROOF_FINGERPRINT=ignored
PROOF_RECEIPT_WRITER=ignored
PROOF_SOURCE_BASELINE={shlex.quote(str(baseline))}
PROOF_STAGE_RECEIPT={shlex.quote(str(stages))}
PROOF_RECEIPT={shlex.quote(str(receipt))}
set +e
( run_proof_stage sample fail_then_succeed )
status=$?
set -e
test "$status" -ne 0
grep -q $'^sample\\tfail\\t' "$PROOF_STAGE_RECEIPT"
"""
            result = subprocess.run(
                ["bash", "-c", script],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_halmos_prerequisite_checks_fail_closed(self) -> None:
        body = function_body("run_halmos")
        self.assertIn('halmos_environment.py" --check || return', body)
        self.assertIn('check_solidity_ast_bindings.py" --scope bridge || return', body)

    def test_new_modes_are_exposed(self) -> None:
        for mode in (
            "policy",
            "rust-fast",
            "rust-integration",
            "contracts-fast",
            "contracts-coverage",
            "certora",
            "proofs-impacted",
            "ui-fast",
            "ui-e2e",
        ):
            self.assertRegex(SOURCE, rf"(?m)^  {re.escape(mode)}\)$")

    def run_automatic_execution_guard(
        self,
        signer_source: str,
        *,
        additional_source: str = "",
        additional_source_path: str = "other.rs",
        workspace_dependency: str = '"=0.1.1"',
        canister_dependency: str = "{ workspace = true }",
        lock_version: str = "0.1.1",
        lock_source: str = "registry+https://github.com/rust-lang/crates.io-index",
        lock_checksum: str = "92c1319a274caebf0ab70ab826b8905c29e8563498289356b9a59464f2a85c56",
        workspace_suffix: str = "",
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            canister_source = root / "canister" / "bridge-canister" / "src"
            canister_source.mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                "[workspace]\n"
                "members = [\"canister/bridge-canister\"]\n"
                "[workspace.dependencies]\n"
                f"ic-cdk-management-canister = {workspace_dependency}\n"
                f"{workspace_suffix}",
                encoding="utf-8",
            )
            (canister_source.parent / "Cargo.toml").write_text(
                "[package]\n"
                'name = "bridge-canister"\n'
                'version = "0.1.0"\n'
                "[dependencies]\n"
                f"ic-cdk-management-canister = {canister_dependency}\n",
                encoding="utf-8",
            )
            (root / "Cargo.lock").write_text(
                "version = 4\n\n"
                "[[package]]\n"
                'name = "ic-cdk-management-canister"\n'
                f'version = "{lock_version}"\n'
                f'source = "{lock_source}"\n'
                f'checksum = "{lock_checksum}"\n',
                encoding="utf-8",
            )
            (canister_source / "signer.rs").write_text(signer_source, encoding="utf-8")
            if additional_source:
                additional_path = canister_source / additional_source_path
                additional_path.parent.mkdir(parents=True, exist_ok=True)
                additional_path.write_text(
                    additional_source,
                    encoding="utf-8",
                )
            (root / "canister" / "bridge-core").mkdir(parents=True)
            (root / "verification" / "verus").mkdir(parents=True)
            (root / "ui" / "src").mkdir(parents=True)
            script = (
                "set -euo pipefail\n"
                "verify_tecdsa_wrapper_dependency() {\n"
                f"{function_body('verify_tecdsa_wrapper_dependency')}"
                "}\n"
                "run_no_automatic_execution_guards() {\n"
                f"{function_body('run_no_automatic_execution_guards')}"
                "}\n"
                f"ROOT={shlex.quote(str(root))}\n"
                "run_no_automatic_execution_guards\n"
            )
            return subprocess.run(
                ["bash", "-c", script],
                check=False,
                capture_output=True,
                text=True,
            )

    def test_threshold_signing_guard_requires_the_reviewed_call(self) -> None:
        zero_calls = self.run_automatic_execution_guard("fn signer() {}\n")
        self.assertNotEqual(zero_calls.returncode, 0, zero_calls.stderr)

        reviewed_call = self.run_automatic_execution_guard(
            "::ic_cdk_management_canister::sign_with_ecdsa(sign_args)\n"
        )
        self.assertEqual(reviewed_call.returncode, 0, reviewed_call.stderr)

    def test_threshold_signing_guard_rejects_unreviewed_calls(self) -> None:
        cases = {
            "multiple": (
                "::ic_cdk_management_canister::sign_with_ecdsa(sign_args);\n"
                "::ic_cdk_management_canister::sign_with_ecdsa(sign_args);\n",
                "",
                "other.rs",
            ),
            "raw unbounded": (
                '::ic_cdk::call::Call::unbounded_wait(target, "raw_rand")\n',
                "",
                "other.rs",
            ),
            "unqualified wrapper": (
                "sign_with_ecdsa(sign_args)\n",
                "",
                "other.rs",
            ),
            "other source": (
                "fn signer() {}\n",
                "::ic_cdk_management_canister::sign_with_ecdsa(sign_args)\n",
                "other.rs",
            ),
            "nested signer source": (
                "fn signer() {}\n",
                "::ic_cdk_management_canister::sign_with_ecdsa(sign_args)\n",
                "nested/signer.rs",
            ),
            "unformatted reviewed call": (
                "let result = ::ic_cdk_management_canister::sign_with_ecdsa(sign_args);\n",
                "",
                "other.rs",
            ),
            "shadowed dependency": (
                "extern crate replacement as ic_cdk_management_canister;\n"
                "::ic_cdk_management_canister::sign_with_ecdsa(sign_args)\n",
                "",
                "other.rs",
            ),
            "aliased wrapper import": (
                "use ic_cdk_management_canister::sign_with_ecdsa as sign;\n"
                "sign(sign_args);\n",
                "",
                "other.rs",
            ),
            "grouped aliased wrapper import": (
                "use ic_cdk_management_canister::{\n"
                "    sign_with_ecdsa as sign,\n"
                "};\n"
                "sign(sign_args);\n",
                "",
                "other.rs",
            ),
            "wrapper function value": (
                "let sign = ::ic_cdk_management_canister::sign_with_ecdsa;\n"
                "sign(sign_args);\n",
                "",
                "other.rs",
            ),
            "signer heartbeat": (
                "fn heartbeat() {}\n",
                "",
                "other.rs",
            ),
            "signer recurring timer": (
                "fn signer() { set_timer_interval(); }\n",
                "",
                "other.rs",
            ),
        }
        for name, (signer_source, additional_source, additional_source_path) in cases.items():
            with self.subTest(name=name):
                result = self.run_automatic_execution_guard(
                    signer_source,
                    additional_source=additional_source,
                    additional_source_path=additional_source_path,
                )
                self.assertNotEqual(result.returncode, 0, result.stderr)

    def test_threshold_signing_guard_rejects_wrapper_supply_chain_changes(self) -> None:
        reviewed = "::ic_cdk_management_canister::sign_with_ecdsa(sign_args)\n"
        cases = {
            "workspace path": {"workspace_dependency": '{ path = "vendor/wrapper" }'},
            "workspace rename": {
                "workspace_dependency": '{ version = "=0.1.1", package = "replacement" }'
            },
            "canister path": {"canister_dependency": '{ path = "../../replacement" }'},
            "lock version": {"lock_version": "0.1.2"},
            "lock source": {"lock_source": "git+https://example.invalid/wrapper"},
            "lock checksum": {"lock_checksum": "0" * 64},
            "workspace patch": {
                "workspace_suffix": (
                    "\n[patch.crates-io]\n"
                    'ic-cdk-management-canister = { path = "vendor/wrapper" }\n'
                )
            },
        }
        for name, overrides in cases.items():
            with self.subTest(name=name):
                result = self.run_automatic_execution_guard(reviewed, **overrides)
                self.assertNotEqual(result.returncode, 0, result.stderr)

    def test_versions_rejects_npm_lockfiles(self) -> None:
        body = function_body("run_versions")
        self.assertLess(body.index('  verify_no_npm_lockfiles "$ROOT"\n'), body.index("check_tool_versions.sh"))
        self.assertNotIn(
            "run_certora_advisory_checks",
            body,
            "advisory Certora checks must not become an existing release-gate prerequisite",
        )
        guard = GUARDS
        for relative_path in (
            "package-lock.json",
            "npm-shrinkwrap.json",
            "ui/package-lock.json",
            "ui/npm-shrinkwrap.json",
        ):
            self.assertIn(relative_path, guard)

    def test_versions_shellchecks_every_production_release_driver(self) -> None:
        body = function_body("run_versions")
        for relative_path in (
            "production-release.sh",
            "production-validation.sh",
            "production-deploy-driver.sh",
            "production-activate-driver.sh",
            "production-seal-driver.sh",
            "production-handover-driver.sh",
            "production-canister-upgrade.sh",
        ):
            with self.subTest(relative_path=relative_path):
                self.assertIn(f'"$ROOT/scripts/{relative_path}"', body)


if __name__ == "__main__":
    unittest.main()
