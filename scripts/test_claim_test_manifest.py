#!/usr/bin/env python3
"""Regression tests for claim transaction-test registration and execution."""

from __future__ import annotations

import subprocess
import json
import copy
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import check_claim_test_manifest as claim_tests


class ClaimTestManifestTests(unittest.TestCase):
    @staticmethod
    def claims(row: str) -> str:
        return (
            "schema\t7\t-\t-\t-\t-\t-\n"
            "contract\tclaim\timplementation-only\trelease-safety\tproduction-linked\t"
            "BridgeSpec.TestContract\tBridgeSpec.test_witness\n"
            + row
        )

    @staticmethod
    def write_links(root: Path, target: str, symbol: str = "exact_test") -> None:
        path = root / "verification/claim-test-links.tsv"
        path.parent.mkdir(parents=True)
        path.write_text(f"claim\t{target}\t{symbol}\n")

    def fixture(self) -> tuple[Path, str, str]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        target = root / "canister/bridge-core/tests/example.rs"
        target.parent.mkdir(parents=True)
        target.write_text("fn exact_test() {}\n", encoding="utf-8")
        self.write_links(root, "canister/bridge-core/tests/example.rs")
        claims = self.claims(
            "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
            "-\t-\n"
        )
        manifest = (
            "rust-core\tcanister/bridge-core/tests/example.rs\t"
            "exact_test\texact_test\tgrouped\n"
        )
        return root, claims, manifest

    def test_manifest_must_exactly_cover_claim_links(self) -> None:
        root, claims, _ = self.fixture()
        with self.assertRaisesRegex(ValueError, "does not match claims"):
            claim_tests.parse_manifest(claims, "", root)

    def test_separate_links_reject_unknown_duplicate_and_missing_ownership(self) -> None:
        root, claims, manifest = self.fixture()
        links = root / "verification/claim-test-links.tsv"
        original = links.read_text()
        for invalid in ["", original + original, original.replace("claim\t", "unknown\t"), "claim\tbad\n"]:
            links.write_text(invalid)
            with self.subTest(links=invalid), self.assertRaises(ValueError):
                claim_tests.parse_manifest(claims, manifest, root)

    def test_definition_rejects_embedded_test_links_and_old_schema(self) -> None:
        _, claims, _ = self.fixture()
        with self.assertRaisesRegex(ValueError, "schema 7"):
            claim_tests.parse_claim_manifest(claims.replace("schema\t7", "schema\t6"))
        with self.assertRaisesRegex(ValueError, "invalid claim row"):
            claim_tests.parse_claim_manifest(claims.replace("\tp\t-\t-", "\tp\ttest.rs#case\t-\t-"))

    def test_manifest_rejects_missing_symbol(self) -> None:
        root, claims, manifest = self.fixture()
        manifest = manifest.replace("exact_test\texact_test", "missing\texact_test")
        with self.assertRaisesRegex(ValueError, "symbol is missing"):
            claim_tests.parse_manifest(claims, manifest, root)

    def test_manifest_rejects_an_excess_registration(self) -> None:
        root, claims, manifest = self.fixture()
        target = root / "canister/bridge-core/tests/example.rs"
        target.write_text("fn exact_test() {}\nfn extra_test() {}\n", encoding="utf-8")
        manifest += (
            "rust-core\tcanister/bridge-core/tests/example.rs\t"
            "extra_test\textra_test\tgrouped\n"
        )
        with self.assertRaisesRegex(ValueError, "does not match claims"):
            claim_tests.parse_manifest(claims, manifest, root)

    def test_manifest_rejects_an_unsupported_runner(self) -> None:
        root, claims, manifest = self.fixture()
        manifest = manifest.replace("rust-core", "python")
        with self.assertRaisesRegex(ValueError, "unsupported claim test runner target"):
            claim_tests.parse_manifest(claims, manifest, root)

    def test_manifest_accepts_matching_symbol_and_selector(self) -> None:
        root, claims, manifest = self.fixture()
        self.assertEqual(len(claim_tests.parse_manifest(claims, manifest, root)), 1)

    def test_manifest_rejects_selector_not_bound_to_symbol(self) -> None:
        root, claims, manifest = self.fixture()
        target = root / "canister/bridge-core/tests/example.rs"
        target.write_text("fn exact_test() {}\nfn unrelated() {}\n", encoding="utf-8")
        manifest = manifest.replace("exact_test\texact_test", "exact_test\tunrelated")
        with self.assertRaisesRegex(ValueError, "selector is not bound to symbol"):
            claim_tests.parse_manifest(claims, manifest, root)

    def test_manifest_accepts_named_vitest_callback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "ui/src/example.test.ts"
            target.parent.mkdir(parents=True)
            target.write_text(
                'function exact_test() {}\nit("human title", exact_test)\n',
                encoding="utf-8",
            )
            self.write_links(root, "ui/src/example.test.ts")
            claims = self.claims(
                "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
                "-\t-\n"
            )
            manifest = (
                "vitest\tui/src/example.test.ts\texact_test\thuman title\tgrouped\n"
            )
            self.assertEqual(
                len(claim_tests.parse_manifest(claims, manifest, root)), 1
            )

    def test_manifest_accepts_vitest_tsx_target(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "ui/src/example.test.tsx"
            target.parent.mkdir(parents=True)
            target.write_text('it("tsx_test", () => <div />)\n', encoding="utf-8")
            self.write_links(root, "ui/src/example.test.tsx", "tsx_test")
            claims = self.claims(
                "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
                "-\t-\n"
            )
            manifest = "vitest\tui/src/example.test.tsx\ttsx_test\ttsx_test\tgrouped\n"
            self.assertEqual(len(claim_tests.parse_manifest(claims, manifest, root)), 1)

    def test_manifest_accepts_named_jest_callback(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "integration/phase3.spec.ts"
            target.parent.mkdir(parents=True)
            target.write_text(
                'async function exact_test() {}\ntest("human title", exact_test)\n',
                encoding="utf-8",
            )
            self.write_links(root, "integration/phase3.spec.ts")
            claims = self.claims(
                "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
                "-\t-\n"
            )
            manifest = (
                "jest\tintegration/phase3.spec.ts\texact_test\thuman title\tgrouped\n"
            )
            self.assertEqual(
                len(claim_tests.parse_manifest(claims, manifest, root)), 1
            )

    def test_rust_execution_requires_exactly_one_pass(self) -> None:
        test = claim_tests.ClaimTest(
            "rust-core", "canister/bridge-core/tests/example.rs", "exact_test", "exact_test"
        )

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            command = _args[0]
            output = "exact_test: test\n" if "--list" in command else "running 0 tests\n"
            return subprocess.CompletedProcess([], 0, output, "")

        with self.assertRaisesRegex(ValueError, "did not pass exactly once"):
            claim_tests.execute_group([test], Path("."), runner)

    def test_json_report_tolerates_package_manager_warning_prefix(self) -> None:
        report = claim_tests.parse_json_report(
            '[WARN] Unsupported engine: wanted node 24\n{"success":true}\n'
        )
        self.assertEqual(report, {"success": True})

    def test_json_report_rejects_non_json_output(self) -> None:
        with self.assertRaisesRegex(ValueError, "terminal JSON payload"):
            claim_tests.parse_json_report("warning only\n")

    def test_json_report_rejects_duplicate_keys(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate test report key"):
            claim_tests.parse_json_report('{"tests":{"a":"failed","a":"passed"}}')

    def test_test_deployment_runner_enables_the_feature(self) -> None:
        test = claim_tests.ClaimTest(
            "rust-canister-test-deployment",
            "canister/bridge-canister/src/example.rs",
            "exact_test",
            "exact_test",
        )
        commands: list[list[str]] = []

        def runner(
            command: list[str], **_kwargs: object
        ) -> subprocess.CompletedProcess[str]:
            commands.append(command)
            return subprocess.CompletedProcess(
                command, 0,
                "example::tests::exact_test: test\n" if "--list" in command else
                "running 1 test\ntest example::tests::exact_test ... ok\n", ""
            )

        claim_tests.execute_group([test], Path("."), runner)
        self.assertEqual(len(commands), 2)
        self.assertIn("--features", commands[0])
        self.assertIn("test-deployment", commands[0])

    def test_jest_dependencies_are_built_once_before_execution(self) -> None:
        tests = [
            claim_tests.ClaimTest(
                "jest",
                "integration/phase3.spec.ts",
                "first_test",
                "first test",
            ),
            claim_tests.ClaimTest(
                "jest",
                "integration/phase3.spec.ts",
                "second_test",
                "second test",
            ),
        ]
        commands: list[list[str]] = []

        def runner(
            command: list[str], **_kwargs: object
        ) -> subprocess.CompletedProcess[str]:
            commands.append(command)
            return subprocess.CompletedProcess(command, 0, "", "")

        root = Path("/tmp/claim-test-root")
        claim_tests.prepare_test_dependencies(tests, root, runner)

        self.assertEqual(len(commands), 3)
        self.assertEqual(
            commands[0],
            [str(root / "scripts/plan007/build-staging-canister-wasm.sh")],
        )
        self.assertEqual(
            commands[1],
            [str(root / "scripts/plan007/build-schema35-predecessor-wasm.sh")],
        )
        self.assertIn("mock-external", commands[2])

    def test_non_jest_dependencies_require_no_build(self) -> None:
        test = claim_tests.ClaimTest(
            "rust-core",
            "canister/bridge-core/tests/example.rs",
            "exact_test",
            "exact_test",
        )

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            self.fail("dependency build should not run without Jest claim tests")

        claim_tests.prepare_test_dependencies([test], Path("."), runner)

    def test_validate_only_does_not_build_or_execute_tests(self) -> None:
        with (
            mock.patch.object(
                claim_tests,
                "prepare_test_dependencies",
                side_effect=AssertionError("validate-only must not build dependencies"),
            ),
            mock.patch.object(
                claim_tests,
                "execute_group",
                side_effect=AssertionError("validate-only must not execute tests"),
            ),
        ):
            self.assertEqual(claim_tests.main(["--validate-only"]), 0)

    def test_old_manifest_shape_and_unknown_policy_are_rejected(self) -> None:
        root, claims, manifest = self.fixture()
        for replacement in ["", "\tunknown", "\tisolated:"]:
            with self.subTest(replacement=replacement), self.assertRaises(ValueError):
                claim_tests.parse_manifest(claims, manifest.replace("\tgrouped", replacement), root)

    def test_grouping_preserves_all_tests_and_isolation(self) -> None:
        tests = [claim_tests.ClaimTest("vitest", "ui/src/a.test.ts", name, name) for name in ["a", "b"]]
        tests.append(claim_tests.ClaimTest("vitest", "ui/src/a.test.ts", "c", "c", "isolated:process-global"))
        tests.append(claim_tests.ClaimTest("vitest", "ui/src/b.test.ts", "d", "d"))
        groups = claim_tests.group_tests(tests)
        self.assertEqual([len(group) for group in groups], [2, 1, 1])
        self.assertCountEqual([test for group in groups for test in group], tests)

    def test_grouping_never_merges_different_rust_features(self) -> None:
        tests = [claim_tests.ClaimTest(runner, "canister/bridge-canister/src/api.rs", "a", "a")
                 for runner in ["rust-canister", "rust-canister-test-deployment"]]
        self.assertEqual(len(claim_tests.group_tests(tests)), 2)

    def test_isolated_policy_requires_a_reason(self) -> None:
        root, claims, manifest = self.fixture()
        parsed = claim_tests.parse_manifest(claims, manifest.replace("grouped", "isolated:process-global"), root)
        self.assertEqual(parsed[0].execution, "isolated:process-global")

    def test_required_results_reject_all_non_exact_pass_sets(self) -> None:
        for results in [[], [("a", "passed")], [("a", "passed"), ("a", "passed")],
                        [("a", "passed"), ("b", "ignored")],
                        [("a", "passed"), ("b", "failed")],
                        [("a", "passed"), ("b", "passed"), ("extra", "passed")]]:
            with self.subTest(results=results), self.assertRaises(ValueError):
                claim_tests.check_passes(["a", "b"], results)

    @staticmethod
    def js_report(names: list[str], target: str = "/tmp/claim-test-root/ui/src/a.test.ts") -> dict:
        return {"success": True, "numPassedTests": len(names), "numFailedTests": 0,
                "testResults": [{"name": target, "assertionResults": [
                    {"title": name, "fullName": "suite " + name, "ancestorTitles": ["suite"], "status": "passed"}
                    for name in names]}]}

    def test_grouped_javascript_runs_once_and_escapes_literal_titles(self) -> None:
        root = Path("/tmp/claim-test-root")
        for runner_name, target in [("vitest", "ui/src/a.test.ts"), ("jest", "integration/phase3.spec.ts")]:
            names = ["first (literal)", "second.+"]
            tests = [claim_tests.ClaimTest(runner_name, target, f"test_{index}", name) for index, name in enumerate(names)]
            commands = []
            def runner(command, **_kwargs):
                commands.append(command)
                payload = json.dumps(self.js_report(names, str(root / target)))
                if "--outputFile" in command:
                    Path(command[command.index("--outputFile") + 1]).write_text(payload)
                return subprocess.CompletedProcess(command, 0, payload, "")
            with self.subTest(runner=runner_name):
                claim_tests.execute_group(tests, root, runner)
                self.assertEqual(len(commands), 1)
                self.assertIn(r"first\ \(literal\)", commands[0][commands[0].index("-t") + 1])

    def test_javascript_report_rejects_missing_skipped_duplicate_failed_and_wrong_identity(self) -> None:
        tests = [claim_tests.ClaimTest("vitest", "ui/src/a.test.ts", name, name) for name in ["a", "b"]]
        good = self.js_report(["a", "b"])
        variants = []
        missing = copy.deepcopy(good)
        missing["testResults"][0]["assertionResults"].pop()
        variants.append(missing)
        for status in ["pending", "skipped", "failed", "todo"]:
            mutated = copy.deepcopy(good)
            mutated["testResults"][0]["assertionResults"][0]["status"] = status
            variants.append(mutated)
        duplicate = copy.deepcopy(good)
        duplicate["testResults"][0]["assertionResults"].append(copy.deepcopy(duplicate["testResults"][0]["assertionResults"][0]))
        variants.append(duplicate)
        wrong_file = copy.deepcopy(good)
        wrong_file["testResults"][0]["name"] = "/tmp/wrong.test.ts"
        variants.append(wrong_file)
        wrong_name = copy.deepcopy(good)
        wrong_name["testResults"][0]["assertionResults"][0]["fullName"] = "different name"
        variants.append(wrong_name)
        variants += [None, {}, self.js_report(["a", "b", "extra"])]
        for report in variants:
            with self.subTest(report=report), self.assertRaises(ValueError):
                claim_tests.check_js_report(report, tests, Path("/tmp/claim-test-root"))

    def test_javascript_allows_only_unselected_skips(self) -> None:
        tests = [claim_tests.ClaimTest("vitest", "ui/src/a.test.ts", "a", "a")]
        report = self.js_report(["a"])
        report["testResults"][0]["assertionResults"].append({"title": "other", "status": "pending"})
        claim_tests.check_js_report(report, tests, Path("/tmp/claim-test-root"))

    def test_rust_group_resolves_full_names_and_executes_exact_filters_once(self) -> None:
        tests = [claim_tests.ClaimTest("rust-canister", "canister/bridge-canister/src/api.rs", name, name) for name in ["a", "b"]]
        commands = []
        def runner(command, **_kwargs):
            commands.append(command)
            output = "api::tests::a: test\napi::tests::b: test\nother::a: test\n" if "--list" in command else "running 2 tests\ntest api::tests::a ... ok\ntest api::tests::b ... ok\n"
            return subprocess.CompletedProcess(command, 0, output, "")
        claim_tests.execute_group(tests, Path("."), runner)
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[-1][-2:], ["api::tests::a", "api::tests::b"])
        self.assertIn("--exact", commands[-1])

    def test_rust_profile_group_uses_the_profile_package_and_exact_name(self) -> None:
        test = claim_tests.ClaimTest(
            "rust-profile",
            "tools/bridge-profile/src/main.rs",
            "exact_test",
            "exact_test",
        )
        self.assertTrue(claim_tests.runner_accepts(test))
        self.assertFalse(
            claim_tests.runner_accepts(
                claim_tests.ClaimTest("rust-profile", "other.rs", "exact_test", "exact_test")
            )
        )
        commands = []

        def runner(command, **_kwargs):
            commands.append(command)
            output = (
                "tests::exact_test: test\n"
                if "--list" in command
                else "running 1 test\ntest tests::exact_test ... ok\n"
            )
            return subprocess.CompletedProcess(command, 0, output, "")

        claim_tests.execute_group([test], Path("."), runner)
        self.assertEqual(len(commands), 2)
        self.assertEqual(commands[0][commands[0].index("-p") + 1], "bridge-profile")
        self.assertEqual(commands[-1][-1], "tests::exact_test")
        self.assertIn("--exact", commands[-1])

    def test_rust_ambiguous_listing_is_rejected_before_execution(self) -> None:
        test = claim_tests.ClaimTest("rust-canister", "canister/bridge-canister/src/api.rs", "a", "a")
        runner = mock.Mock(return_value=subprocess.CompletedProcess([], 0, "api::tests::a: test\napi::nested::a: test\n", ""))
        with self.assertRaisesRegex(ValueError, "resolve exactly once"):
            claim_tests.execute_group([test], Path("."), runner)
        self.assertEqual(runner.call_count, 1)

    def test_foundry_group_requires_exact_success_set(self) -> None:
        tests = [claim_tests.ClaimTest("foundry", "contracts/test/A.t.sol", name, name) for name in ["testA", "testB"]]
        good = {"test/A.t.sol:A": {"test_results": {name + "()": {"status": "Success"} for name in ["testA", "testB"]}}}
        runner = mock.Mock(return_value=subprocess.CompletedProcess([], 0, json.dumps(good), ""))
        claim_tests.execute_group(tests, Path("."), runner)
        self.assertEqual(runner.call_count, 1)
        good["test/A.t.sol:A"]["test_results"]["testA()"]["status"] = "Skipped"
        runner.return_value = subprocess.CompletedProcess([], 0, json.dumps(good), "")
        with self.assertRaisesRegex(ValueError, "did not pass exactly once"):
            claim_tests.execute_group(tests, Path("."), runner)

    def test_plan_only_never_builds_or_executes(self) -> None:
        with mock.patch.object(claim_tests, "prepare_test_dependencies") as prepare, mock.patch.object(claim_tests, "execute_group") as execute, mock.patch("builtins.print"):
            self.assertEqual(claim_tests.main(["--plan-only"]), 0)
        prepare.assert_not_called()
        execute.assert_not_called()

    def test_impact_json_rejects_duplicate_keys_and_invalid_stage_shapes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "impact.json"
            invalid_plans = [
                '{"claims":["deposit_identity_preflight"],"stages":["claim-transaction-tests"],"stages":["claim-transaction-tests"],"unregistered":[]}',
                '{"claims":["deposit_identity_preflight"],"stages":"claim-transaction-tests","unregistered":[]}',
                '{"claims":["deposit_identity_preflight"],"stages":["claim-transaction-tests","claim-transaction-tests"],"unregistered":[]}',
            ]
            for plan in invalid_plans:
                path.write_text(plan, encoding="utf-8")
                with self.subTest(plan=plan), self.assertRaises(ValueError):
                    claim_tests.main(["--impact-json", str(path), "--plan-only"])

    def test_claim_selection_is_complete_and_deduplicates_shared_tests(self) -> None:
        root, claims, manifest = self.fixture()
        tests = claim_tests.parse_manifest(claims, manifest, root)
        self.assertEqual(claim_tests.select_claim_tests(claims, tests, ["claim"]), tests)
        with self.assertRaisesRegex(ValueError, "every required test"):
            claim_tests.select_claim_tests(claims, [], ["claim"])
        for selected in [[], ["unknown"], ["claim", "claim"], "claim", [1]]:
            with self.subTest(selected=selected), self.assertRaises(ValueError):
                claim_tests.select_claim_tests(claims, tests, selected)

    def test_ui_only_claim_selection_does_not_prepare_wasm(self) -> None:
        claims = claim_tests.CLAIMS.read_text()
        tests = claim_tests.parse_manifest(claims, claim_tests.MANIFEST.read_text())
        synthetic = self.claims("kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t-\t-\n")
        ui_test = claim_tests.ClaimTest("vitest", "ui/src/a.test.ts", "a", "a", claims=("claim",))
        integration = claim_tests.ClaimTest("jest", "integration/phase3.spec.ts", "b", "b")
        selected = claim_tests.select_claim_tests(synthetic, [ui_test, integration], ["claim"])
        runner = mock.Mock(side_effect=AssertionError("UI selection must not build Wasm"))
        claim_tests.prepare_test_dependencies(selected, runner=runner)
        runner.assert_not_called()
        selected_live = claim_tests.select_claim_tests(claims, tests, ["deposit_identity_preflight"])
        links = claim_tests.parse_claim_test_links(claims, claim_tests.LINKS.read_text())
        required = {tuple(link.split("#", 1)) for link in links["deposit_identity_preflight"]}
        self.assertEqual({(test.target, test.symbol) for test in selected_live}, required)

    def test_full_stage_never_uses_an_impacted_selection(self) -> None:
        source = (claim_tests.ROOT / "scripts/ci-local.sh").read_text()
        self.assertIn('python3 "$CLAIM_TEST_CHECK" --impact-json "$impact_json"', source)
        full = source[source.index("run_proofs() {"):]
        full = full[:full.index("\n}")]
        self.assertNotIn("--impact-json", full)

    def test_impacted_shell_dispatch_passes_claim_plan_to_test_stage_only(self) -> None:
        source = (claim_tests.ROOT / "scripts/ci-local.sh").read_text()
        function = source[source.index("run_impacted_proofs() {"):]
        function = function[:function.index("\n}") + 2]
        with tempfile.TemporaryDirectory() as directory:
            script = """set -euo pipefail
TMP_ROOT="$1"
initialize_proof_context() { PROOF_IMPACT_CHECK=impact; CLAIM_TEST_CHECK=tests; }
python3() {
  if [[ "$1" == impact ]]; then
    echo '{"claims":["deposit_identity_preflight"],"stages":["claim-manifest","claim-transaction-tests"],"unregistered":[]}'
  elif [[ "$1" == -c ]]; then
    printf 'claim-manifest\\nclaim-transaction-tests\\n'
  else
    printf 'test-command:%s\\n' "$*"
  fi
}
run_proof_stage_command() { printf 'other-stage:%s\\n' "$1"; }
""" + function + '\nrun_impacted_proofs paths.json\n'
            result = subprocess.run(["bash", "-c", script, "test", directory], capture_output=True, text=True, check=True)
        self.assertIn("other-stage:claim-manifest", result.stdout)
        self.assertNotIn("other-stage:claim-transaction-tests", result.stdout)
        commands = [line for line in result.stdout.splitlines() if line.startswith("test-command:")]
        self.assertEqual(commands, [f"test-command:tests --impact-json {directory}/proof-impact.json --plan-only",
                                    f"test-command:tests --impact-json {directory}/proof-impact.json"])


if __name__ == "__main__":
    unittest.main()
