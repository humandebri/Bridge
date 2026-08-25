#!/usr/bin/env python3
"""Regression tests for claim transaction-test registration and execution."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

import check_claim_test_manifest as claim_tests


class ClaimTestManifestTests(unittest.TestCase):
    @staticmethod
    def claims(row: str) -> str:
        return (
            "schema\t6\t-\t-\t-\t-\t-\n"
            "contract\tclaim\timplementation-only\trelease-safety\tproduction-linked\t"
            "BridgeSpec.TestContract\tBridgeSpec.test_witness\n"
            + row
        )

    def fixture(self) -> tuple[Path, str, str]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        target = root / "canister/bridge-core/tests/example.rs"
        target.parent.mkdir(parents=True)
        target.write_text("fn exact_test() {}\n", encoding="utf-8")
        claims = self.claims(
            "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
            "canister/bridge-core/tests/example.rs#exact_test\t-\t-\n"
        )
        manifest = (
            "rust-core\tcanister/bridge-core/tests/example.rs\t"
            "exact_test\texact_test\n"
        )
        return root, claims, manifest

    def test_manifest_must_exactly_cover_claim_links(self) -> None:
        root, claims, _ = self.fixture()
        with self.assertRaisesRegex(ValueError, "does not match claims"):
            claim_tests.parse_manifest(claims, "", root)

    def test_manifest_rejects_missing_symbol(self) -> None:
        root, claims, manifest = self.fixture()
        manifest = manifest.replace("exact_test\texact_test", "missing\texact_test")
        with self.assertRaisesRegex(ValueError, "symbol is missing"):
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
            claims = self.claims(
                "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
                "ui/src/example.test.ts#exact_test\t-\t-\n"
            )
            manifest = (
                "vitest\tui/src/example.test.ts\texact_test\thuman title\n"
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
            claims = self.claims(
                "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
                "ui/src/example.test.tsx#tsx_test\t-\t-\n"
            )
            manifest = "vitest\tui/src/example.test.tsx\ttsx_test\ttsx_test\n"
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
            claims = self.claims(
                "kind\tclaim\ta\t-\t-\tv\t-\t-\t-\tp\t"
                "integration/phase3.spec.ts#exact_test\t-\t-\n"
            )
            manifest = (
                "jest\tintegration/phase3.spec.ts\texact_test\thuman title\n"
            )
            self.assertEqual(
                len(claim_tests.parse_manifest(claims, manifest, root)), 1
            )

    def test_rust_execution_requires_exactly_one_pass(self) -> None:
        test = claim_tests.ClaimTest(
            "rust-core", "canister/bridge-core/tests/example.rs", "exact_test", "exact_test"
        )

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            return subprocess.CompletedProcess([], 0, "running 0 tests\n", "")

        with self.assertRaisesRegex(ValueError, "did not pass exactly once"):
            claim_tests.execute_test(test, Path("."), runner)

    def test_json_report_tolerates_package_manager_warning_prefix(self) -> None:
        report = claim_tests.parse_json_report(
            '[WARN] Unsupported engine: wanted node 24\n{"success":true}\n'
        )
        self.assertEqual(report, {"success": True})

    def test_json_report_rejects_non_json_output(self) -> None:
        with self.assertRaisesRegex(ValueError, "terminal JSON payload"):
            claim_tests.parse_json_report("warning only\n")

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
                command, 0, "test module::exact_test ... ok\n", ""
            )

        claim_tests.execute_test(test, Path("."), runner)
        self.assertEqual(len(commands), 1)
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

        self.assertEqual(len(commands), 2)
        self.assertEqual(
            commands[0],
            [str(root / "scripts/plan007/build-staging-canister-wasm.sh")],
        )
        self.assertIn("mock-external", commands[1])

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

    def test_live_manifest_parses(self) -> None:
        parsed = claim_tests.parse_manifest(
            claim_tests.CLAIMS.read_text(encoding="utf-8"),
            claim_tests.MANIFEST.read_text(encoding="utf-8"),
        )
        self.assertGreater(len(parsed), 0)


if __name__ == "__main__":
    unittest.main()
