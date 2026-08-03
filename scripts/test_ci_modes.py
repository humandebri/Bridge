#!/usr/bin/env python3
"""Regression tests for local CI mode composition and proof layout selection."""

from pathlib import Path
import re
import subprocess
import tempfile
import unittest


SOURCE = (Path(__file__).parent / "ci-local.sh").read_text(encoding="utf-8")


def function_body(name: str) -> str:
    match = re.search(rf"^{re.escape(name)}\(\) \{{\n(?P<body>.*?)^\}}$", SOURCE, re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError(f"missing function: {name}")
    return match.group("body")


def function_source(name: str) -> str:
    match = re.search(
        rf"^{re.escape(name)}\(\) \{{\n.*?^\}}$",
        SOURCE,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"missing function: {name}")
    return match.group(0)


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

    def run_proof_layout_fixture(self, present: tuple[str, ...]) -> list[str]:
        selector = function_source("select_proof_stage_layout")
        proofs = function_source("run_proofs")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "root"
            scripts = root / "scripts"
            scripts.mkdir(parents=True)
            for helper in present:
                (scripts / helper).touch()
            temporary = Path(directory) / "tmp"
            temporary.mkdir()
            harness = f"""
{selector}
{proofs}
run_proof_stage() {{ printf 'stage:%s\\n' "$1"; }}
python3() {{ printf 'python:%s\\n' "$1"; }}
ROOT="$1"
TMP_ROOT="$2"
PROOF_RECEIPT="$2/proof-receipt.json"
run_proofs
"""
            result = subprocess.run(
                ["bash", "-c", harness, "bash", str(root), str(temporary)],
                check=True,
                capture_output=True,
                text=True,
            )
            return result.stdout.splitlines()

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

    def test_proofs_execute_selected_claim_stage_layout(self) -> None:
        body = function_body("run_proofs")
        self.assertIn('proof_stage_layout="$(select_proof_stage_layout "$ROOT")"', body)
        self.assertIn("test_write_proof_receipt.py", body)
        self.assertIn("check_claim_test_manifest.py", body)
        self.assertIn('run_proof_stage claim-manifest python3', body)
        self.assertIn('run_proof_stage claim-transaction-tests', body)
        self.assertIn('python3 "$ROOT/scripts/write_proof_receipt.py"', body)

        legacy = self.run_proof_layout_fixture(())
        independent = self.run_proof_layout_fixture(
            ("test_write_proof_receipt.py", "check_claim_test_manifest.py")
        )
        self.assertTrue(legacy[0].endswith("/scripts/write_proof_receipt.py"))
        self.assertTrue(independent[0].endswith("/scripts/test_write_proof_receipt.py"))
        self.assertEqual(
            [line for line in legacy if line.startswith("stage:")],
            [
                "stage:lean",
                "stage:lean-negative",
                "stage:policy-vector-consumers",
                "stage:refinement-gate",
                "stage:known-answer-consumers",
                "stage:smt-and-negative",
                "stage:verus-and-negative",
            ],
        )
        self.assertEqual(
            [line for line in independent if line.startswith("stage:")],
            [
                "stage:claim-manifest",
                "stage:lean",
                "stage:lean-negative",
                "stage:policy-vector-consumers",
                "stage:refinement-gate",
                "stage:claim-transaction-tests",
                "stage:known-answer-consumers",
                "stage:smt-and-negative",
                "stage:verus-and-negative",
            ],
        )

    def test_proof_stage_layout_selection_uses_complete_helper_pair(self) -> None:
        selector = function_source("select_proof_stage_layout")
        helper_names = (
            "test_write_proof_receipt.py",
            "check_claim_test_manifest.py",
        )
        cases = {
            "legacy-empty": ((), "legacy"),
            "legacy-receipt-only": ((helper_names[0],), "legacy"),
            "legacy-claim-tests-only": ((helper_names[1],), "legacy"),
            "independent-claims": (helper_names, "independent-claims"),
        }
        for name, (present, expected) in cases.items():
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                scripts = root / "scripts"
                scripts.mkdir()
                for helper in present:
                    (scripts / helper).touch()
                result = subprocess.run(
                    [
                        "bash",
                        "-c",
                        f'{selector}\nselect_proof_stage_layout "$1"',
                        "bash",
                        str(root),
                    ],
                    check=True,
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.stdout.strip(), expected)

    def test_proof_stage_stops_on_the_first_failed_command(self) -> None:
        body = function_body("run_proof_stage")
        self.assertIn('  (\n    set -e\n    "$@"\n  )\n', body)
        refinement = function_body("run_refinement_gate")
        self.assertIn('generate_refinement_harness.py" --check || return', refinement)
        commands = [
            line.strip()
            for line in refinement.splitlines()
            if line.strip().startswith("python3")
        ]
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
