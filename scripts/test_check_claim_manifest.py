#!/usr/bin/env python3
"""Regression tests for the claim-manifest Solidity refinement gate."""

import unittest
import subprocess
import tempfile
from pathlib import Path

from claim_manifest import lean_contract_check_source, parse_claim_manifest
from check_claim_manifest import abstract_evidence_status, missing_scalar_calls, solidity_function_body


class ClaimContractTests(unittest.TestCase):
    def manifest(self, contract: str, witness: str) -> str:
        return (
            "schema\t3\t-\t-\t-\n"
            f"contract\tclaim_id\thistory-safety\t{contract}\t{witness}\n"
            "protocol\tclaim_id\tclaim_theorem\t-\ttrace_theorem\t-\t-\t"
            "source.rs#kernel\ttest.rs#case\tassumption\t-\n"
        )

    def test_contract_source_checks_the_expected_type(self) -> None:
        manifest = parse_claim_manifest(
            self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        )
        source = lean_contract_check_source(manifest)
        self.assertIn(
            "example : BridgeSpec.Contract := BridgeSpec.witness", source
        )

    def test_contract_and_witness_cannot_be_declared_independently(self) -> None:
        with self.assertRaisesRegex(ValueError, "must be paired"):
            parse_claim_manifest(self.manifest("BridgeSpec.Contract", "-"))

    def test_every_claim_requires_one_contract_registration(self) -> None:
        document = self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        document = document.replace("contract\tclaim_id", "contract\tother_claim")
        with self.assertRaisesRegex(ValueError, "coverage differs"):
            parse_claim_manifest(document)

    def test_rejects_literal_true_contract(self) -> None:
        with self.assertRaisesRegex(ValueError, "vacuous Lean claim contract"):
            parse_claim_manifest(self.manifest("True", "True.intro"))

    def test_cross_claim_witness_does_not_typecheck(self) -> None:
        root = Path(__file__).resolve().parents[1]
        source = """import BridgeSpec.ClaimContracts
open BridgeSpec.ClaimContracts
example : SettlementBacking := payment_identity_witness
"""
        with tempfile.NamedTemporaryFile(mode="w", suffix=".lean", encoding="utf-8") as check:
            check.write(source)
            check.flush()
            result = subprocess.run(
                ["lake", "env", "lean", check.name],
                cwd=root / "verification" / "lean",
                capture_output=True,
                text=True,
                check=False,
            )
        self.assertNotEqual(result.returncode, 0)


class SolidityFunctionBodyTests(unittest.TestCase):
    def test_abstract_evidence_distinguishes_absent_theorems(self) -> None:
        self.assertEqual(abstract_evidence_status("-"), "not-applicable")
        self.assertEqual(abstract_evidence_status("proved_theorem"), "proved")

    def test_extracts_only_the_named_function_with_nested_braces(self) -> None:
        source = """
function evaluateMint(uint256 value) internal pure returns (uint256) {
    if (value > 0) {
        return value;
    }
    return 0;
}

function feeWithinBounds() internal pure returns (bool) {
    return true;
}
"""
        body = solidity_function_body(source, "evaluateMint")
        self.assertIn("if (value > 0) {", body)
        self.assertNotIn("function feeWithinBounds", body)

    def test_following_helper_declaration_does_not_satisfy_call_requirement(self) -> None:
        source = """
function evaluateMint() internal pure {
    deadlineAccepts(0, 0);
}

function feeWithinBounds() internal pure returns (bool) {
    return true;
}
"""
        body = solidity_function_body(source, "evaluateMint")
        self.assertIn("feeWithinBounds(", missing_scalar_calls(body))

    def test_rejects_missing_function(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing Solidity function"):
            solidity_function_body("function other() internal {}", "evaluateMint")

    def test_rejects_declaration_without_body(self) -> None:
        with self.assertRaisesRegex(ValueError, "has no body"):
            solidity_function_body("function evaluateMint();", "evaluateMint")

    def test_rejects_unbalanced_body(self) -> None:
        with self.assertRaisesRegex(ValueError, "not balanced"):
            solidity_function_body(
                "function evaluateMint() internal { if (true) { }",
                "evaluateMint",
            )


if __name__ == "__main__":
    unittest.main()
