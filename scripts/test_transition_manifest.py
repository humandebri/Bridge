#!/usr/bin/env python3

import unittest

from check_transition_manifest import (
    body_calls,
    check_production_call_site,
    function_body,
    production_body_calls,
    require_exact_coverage,
    rust_function_body,
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

    def test_production_calls_accept_supported_rust_paths(self) -> None:
        self.assertTrue(production_body_calls("{ crate::transition(1); }", "transition"))
        self.assertTrue(production_body_calls("{ bridge_core::transition(1); }", "transition"))
        self.assertTrue(
            production_body_calls(
                "{ self::transition(1); }", "transition", kernel_internal=True
            )
        )
        self.assertTrue(
            production_body_calls(
                "{ transition_body!(1); }", "transition", kernel_internal=True
            )
        )

    def test_production_calls_reject_unqualified_and_shadowed_symbols(self) -> None:
        self.assertFalse(production_body_calls("{ transition(1); }", "transition"))
        self.assertFalse(
            production_body_calls(
                "{ let transition = |_| 1; transition(1); }", "transition"
            )
        )
        self.assertFalse(
            production_body_calls("{ module::crate::transition(1); }", "transition")
        )

    def test_nested_function_declaration_is_not_a_production_call(self) -> None:
        self.assertFalse(
            production_body_calls(
                "{ fn transition(value: u8) -> u8 { value } }", "transition"
            )
        )

    def test_strings_cannot_forge_production_calls(self) -> None:
        source = r'''fn registered() {
let normal = "crate::transition(1)";
let raw = r#"bridge_core::transition(1)"#;
let bytes = b"crate::transition(1)";
}'''
        self.assertFalse(
            production_body_calls(function_body(source, "registered"), "transition")
        )

    def test_production_comments_and_other_functions_do_not_count(self) -> None:
        source = """
fn registered() { /* crate::transition(1); */ }
fn other() { crate::transition(1); }
"""
        self.assertFalse(
            production_body_calls(function_body(source, "registered"), "transition")
        )

    def test_missing_production_call_site_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing production call-site file"):
            check_production_call_site("missing.rs#apply", "transition")

    def test_rust_function_body_stops_before_later_visibility_forms(self) -> None:
        source = """
fn registered() { crate::transition(1); }
pub(crate) fn later() { crate::other(1); }
"""
        body = rust_function_body(source, "registered")
        self.assertTrue(production_body_calls(body, "transition"))
        self.assertFalse(production_body_calls(body, "other"))


if __name__ == "__main__":
    unittest.main()
