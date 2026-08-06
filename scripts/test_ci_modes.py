#!/usr/bin/env python3
"""Static regression tests for local CI mode composition."""

from pathlib import Path
import re
import unittest


SOURCE = (Path(__file__).parent / "ci-local.sh").read_text(encoding="utf-8")


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

    def test_legacy_aggregate_modes_remain_complete(self) -> None:
        self.assert_calls("run_rust", ["run_rust_fast", "run_rust_integration"])
        self.assert_calls("run_contracts", ["run_contracts_fast", "run_contracts_coverage"])
        self.assert_calls("run_ui", ["run_ui_fast", "run_ui_e2e"])

    def test_complete_checks_keep_all_component_aggregates(self) -> None:
        self.assert_calls(
            "run_checks",
            ["run_versions", "run_rust", "run_contracts", "run_proofs", "run_ui", "run_icp_build"],
        )

    def test_proofs_use_independent_claim_stages(self) -> None:
        body = function_body("run_proofs")
        receipt_regression = body.index('python3 "$ROOT/scripts/test_write_proof_receipt.py"')
        claim_manifest = body.index(
            'run_proof_stage claim-manifest python3 "$ROOT/scripts/check_claim_manifest.py"'
        )
        self.assertLess(receipt_regression, claim_manifest)
        self.assertIn('python3 "$ROOT/scripts/test_claim_test_manifest.py"', body)
        self.assertIn('python3 "$ROOT/scripts/test_check_claim_manifest.py"', body)
        self.assertIn("run_proof_stage claim-transaction-tests", body)

    def test_proof_stage_stops_on_the_first_failed_command(self) -> None:
        body = function_body("run_proof_stage")
        self.assertIn('  (\n    set -e\n    "$@"\n  )\n', body)
        refinement = function_body("run_refinement_gate")
        commands = [line.strip() for line in refinement.splitlines() if line.strip()]
        self.assertTrue(all(command.endswith("|| return") for command in commands[:-1]))

    def test_new_modes_are_exposed(self) -> None:
        for mode in (
            "rust-fast",
            "rust-integration",
            "contracts-fast",
            "contracts-coverage",
            "ui-fast",
            "ui-e2e",
        ):
            self.assertRegex(SOURCE, rf"(?m)^  {re.escape(mode)}\)$")


if __name__ == "__main__":
    unittest.main()
