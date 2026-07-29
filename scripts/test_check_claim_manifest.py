#!/usr/bin/env python3
"""Regression tests for the claim-manifest Solidity refinement gate."""

import unittest

from check_claim_manifest import missing_scalar_calls, solidity_function_body


class SolidityFunctionBodyTests(unittest.TestCase):
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
