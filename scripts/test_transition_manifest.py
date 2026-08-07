#!/usr/bin/env python3

import unittest

from check_transition_manifest import (
    body_calls,
    function_body,
    require_exact_coverage,
    strip_comments_and_strings,
)


class TransitionCoverageTests(unittest.TestCase):
    def test_exact_coverage_passes(self) -> None:
        require_exact_coverage({"deposit_transition"}, {"deposit_transition"})

    def test_unregistered_transition_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing=.*withdrawal_transition"):
            require_exact_coverage(
                {"deposit_transition", "withdrawal_transition"},
                {"deposit_transition"},
            )

    def test_removed_transition_registration_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "extra=.*obsolete_transition"):
            require_exact_coverage(set(), {"obsolete_transition"})

    def test_extracts_only_registered_proof_body(self) -> None:
        source = """
fn proof() { if true { kernel::deposit_transition(0, 0); } }
fn other() { kernel::withdrawal_transition(0, 0); }
"""
        body = function_body(source, "proof")
        self.assertTrue(body_calls(body, "deposit_transition"))
        self.assertFalse(body_calls(body, "withdrawal_transition"))

    def test_comment_and_string_do_not_count_as_calls(self) -> None:
        source = '''fn proof() {
// kernel::deposit_transition(0, 0)
let text = "kernel::deposit_transition(0, 0)";
}'''
        self.assertFalse(body_calls(function_body(source, "proof"), "deposit_transition"))

    def test_nested_comments_and_braces_are_ignored(self) -> None:
        source = "fn proof() { /* outer { /* inner } */ } */ kernel::x(); }"
        cleaned = strip_comments_and_strings(source)
        self.assertTrue(body_calls(function_body(cleaned, "proof"), "x"))


if __name__ == "__main__":
    unittest.main()
