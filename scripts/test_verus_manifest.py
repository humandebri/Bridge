#!/usr/bin/env python3
"""Regression tests for typed Verus strength and production binding checks."""

import unittest

from check_transition_manifest import production_body_calls, strip_comments_and_strings
from check_verus_manifest import (
    production_call_site_path,
    rust_body,
    validate_derived_dependencies,
    validate_proof_binding,
    validate_shared_expression,
    verus_spec_body,
)
from verus_manifest import parse_verus_manifest


class VerusManifestParserTests(unittest.TestCase):
    @staticmethod
    def manifest(row: str) -> str:
        return "schema\t4\t-\t-\t-\t-\t-\t-\t-\n" + row + "\tclaim\n"

    def test_shared_expression_requires_one_macro(self) -> None:
        with self.assertRaisesRegex(ValueError, "must name one macro"):
            parse_verus_manifest(
                self.manifest("shared\tshared-expression\tkernel\tproof\tfail.rs\t-\t-\tsrc.rs#caller")
            )

    def test_derived_requires_dependencies_and_is_not_production_bound(self) -> None:
        obligation = parse_verus_manifest(
            self.manifest("derived\tderived\tkernel\tproof\tfail.rs\tbase_kernel\t-\tsrc.rs#caller")
        )["derived"]
        self.assertEqual(obligation.binding, ("base_kernel",))
        self.assertFalse(obligation.production_bound)

    def test_model_cannot_claim_a_production_call_site(self) -> None:
        with self.assertRaisesRegex(ValueError, "cannot bind production"):
            parse_verus_manifest(
                self.manifest("model\tmodel\tkernel\tproof\tfail.rs\t-\t-\tsrc.rs#caller")
            )

    def test_derived_binding_is_limited_to_shared_expression(self) -> None:
        with self.assertRaisesRegex(ValueError, "only shared-expression"):
            parse_verus_manifest(
                self.manifest("derived\tderived\tkernel\tproof\tfail.rs\tbase\t0:0:x > 0\t-")
            )

    def test_rejects_duplicate_derived_production_position(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate Verus derived binding position"):
            parse_verus_manifest(
                self.manifest(
                    "shared\tshared-expression\tkernel\tproof\tfail.rs\tmacro_name\t"
                    "0:0:x > 0;0:1:y > 0\tsrc.rs#caller"
                )
            )

    def test_rejects_duplicate_proof_names(self) -> None:
        document = (
            "schema\t4\t-\t-\t-\t-\t-\t-\t-\n"
            "first\tmodel\tfirst_kernel\tshared_proof\tfirst.rs\t-\t-\t-\tclaim\n"
            "second\tmodel\tsecond_kernel\tshared_proof\tsecond.rs\t-\t-\t-\tclaim\n"
        )
        with self.assertRaisesRegex(ValueError, "duplicate Verus proof"):
            parse_verus_manifest(document)

    def test_rejects_duplicate_claim_ids(self) -> None:
        document = self.manifest(
            "model\tmodel\tkernel\tproof\tfail.rs\t-\t-\t-"
        ).replace("\tclaim\n", "\tclaim;claim\n")
        with self.assertRaisesRegex(ValueError, "duplicate Verus claim IDs"):
            parse_verus_manifest(document)


class VerusDerivedDependencyTests(unittest.TestCase):
    @staticmethod
    def document(*rows: str) -> str:
        return "schema\t4\t-\t-\t-\t-\t-\t-\t-\n" + "\n".join(rows) + "\n"

    @staticmethod
    def row(
        obligation: str,
        kind: str,
        kernel: str,
        proof: str,
        binding: str,
        calls: str | None = None,
    ) -> str:
        if calls is None:
            calls = "src.rs#caller" if kind in {"executable", "shared-expression"} else "-"
        return (
            f"{obligation}\t{kind}\t{kernel}\t{proof}\tfail.rs\t{binding}\t-\t"
            f"{calls}\tclaim"
        )

    def test_accepts_direct_dependency_kinds_even_when_supporting_only(self) -> None:
        obligations = parse_verus_manifest(
            self.document(
                self.row(
                    "base",
                    "shared-expression",
                    "base_kernel",
                    "base_proof",
                    "macro",
                    calls="-",
                ),
                self.row("derived", "derived", "derived_kernel", "derived_proof", "base_kernel"),
            )
        )
        validate_derived_dependencies(obligations)

    def test_rejects_unknown_dependency(self) -> None:
        obligations = parse_verus_manifest(
            self.document(
                self.row("derived", "derived", "derived_kernel", "derived_proof", "missing")
            )
        )
        with self.assertRaisesRegex(ValueError, "unknown dependencies"):
            validate_derived_dependencies(obligations)

    def test_rejects_duplicate_dependency(self) -> None:
        obligations = parse_verus_manifest(
            self.document(
                self.row("base", "executable", "base_kernel", "base_proof", "direct"),
                self.row("derived", "derived", "derived_kernel", "derived_proof", "base_kernel;base_kernel"),
            )
        )
        with self.assertRaisesRegex(ValueError, "duplicate dependencies"):
            validate_derived_dependencies(obligations)

    def test_rejects_derived_and_model_dependencies(self) -> None:
        for dependency_kind, binding in (("derived", "base_kernel"), ("model", "-")):
            with self.subTest(kind=dependency_kind):
                obligations = parse_verus_manifest(
                    self.document(
                        self.row("base", dependency_kind, "base_kernel", "base_proof", binding),
                        self.row("child", "derived", "child_kernel", "child_proof", "base_kernel"),
                    )
                )
                with self.assertRaisesRegex(ValueError, "must be executable or shared-expression"):
                    validate_derived_dependencies(obligations)


class VerusBindingTests(unittest.TestCase):
    @staticmethod
    def shared_source(
        production_call: str,
        specification_call: str,
        *,
        production_parameters: str = "first: u64, second: u64",
        specification_parameters: str = "left: int, right: int",
        specification_prefix: str = "",
        macro_arms: str = "($first:expr, $second:expr) => { $first == $second };",
    ) -> str:
        return f"""
macro_rules! shared_body {{
    {macro_arms}
}}
pub const fn kernel({production_parameters}) -> bool {{
    {production_call}
}}
verus! {{
    pub open spec fn kernel_spec({specification_parameters}) -> bool {{
        {specification_prefix}
        {specification_call}
    }}
}}
"""

    def test_accepts_positional_parameters_and_equivalent_constant_aliases(self) -> None:
        source = self.shared_source(
            "shared_body!(first, u64::MAX)",
            "shared_body!(left, maximum)",
            specification_prefix="let maximum: int = 18446744073709551615;",
        )
        validate_shared_expression(source, "kernel", "shared_body", ())

    def test_accepts_derived_production_expression_as_specification_input(self) -> None:
        source = self.shared_source(
            "shared_body!(first > 0, second == 1)",
            "shared_body!(left, right)",
            production_parameters="first: u64, second: u64",
            specification_parameters="left: bool, right: bool",
        )
        validate_shared_expression(
            source,
            "kernel",
            "shared_body",
            ((0, 0, "first>0"), (1, 1, "second==1")),
        )

    def test_rejects_unregistered_derived_production_expression(self) -> None:
        source = self.shared_source(
            "shared_body!(first > 0, second == 1)",
            "shared_body!(left, right)",
            specification_parameters="left: bool, right: bool",
        )
        with self.assertRaisesRegex(ValueError, "argument binding differs"):
            validate_shared_expression(source, "kernel", "shared_body", ())

    def test_rejects_unused_derived_binding(self) -> None:
        source = self.shared_source(
            "shared_body!(first, second)",
            "shared_body!(left, right)",
        )
        with self.assertRaisesRegex(ValueError, "unused shared-expression derived bindings"):
            validate_shared_expression(
                source, "kernel", "shared_body", ((0, 0, "first > 0"),)
            )

    def test_rejects_multiple_macro_arms(self) -> None:
        source = self.shared_source(
            "shared_body!(first, second)",
            "shared_body!(left, right)",
            macro_arms=(
                "($first:expr, $second:expr) => { $first == $second };"
                "($first:expr) => { $first };"
            ),
        )
        with self.assertRaisesRegex(ValueError, "exactly one arm"):
            validate_shared_expression(source, "kernel", "shared_body", ())

    def test_rejects_multiple_macro_invocations(self) -> None:
        source = self.shared_source(
            "shared_body!(first, second) && shared_body!(second, first)",
            "shared_body!(left, right)",
        )
        with self.assertRaisesRegex(ValueError, "exactly once"):
            validate_shared_expression(source, "kernel", "shared_body", ())

    def test_rejects_argument_count_mismatch(self) -> None:
        source = self.shared_source(
            "shared_body!(first, second)",
            "shared_body!(left)",
        )
        with self.assertRaisesRegex(ValueError, "argument count differs"):
            validate_shared_expression(source, "kernel", "shared_body", ())

    def test_rejects_direct_parameter_reordering(self) -> None:
        source = self.shared_source(
            "shared_body!(first, second)",
            "shared_body!(right, left)",
        )
        with self.assertRaisesRegex(ValueError, "argument binding differs"):
            validate_shared_expression(source, "kernel", "shared_body", ())

    def test_rejects_constant_alias_value_mismatch(self) -> None:
        source = self.shared_source(
            "shared_body!(first, 1u64)",
            "shared_body!(left, one)",
            specification_prefix="let one: int = 2;",
        )
        with self.assertRaisesRegex(ValueError, "argument binding differs"):
            validate_shared_expression(source, "kernel", "shared_body", ())

    def test_comments_declarations_and_other_functions_do_not_count(self) -> None:
        source = strip_comments_and_strings(
            """
fn caller() { /* crate::kernel(); */ }
fn kernel() {}
fn other() { crate::kernel(); }
"""
        )
        body = rust_body(source, "caller")
        self.assertFalse(production_body_calls(body, "kernel"))

    def test_spec_extraction_does_not_include_the_next_spec(self) -> None:
        source = """
pub open spec fn first(value: bool) -> bool { value }
pub open spec fn second() -> bool { first(true) }
"""
        body = verus_spec_body(source, "first")
        self.assertIn("value", body)
        self.assertNotIn("second", body)

    def test_rejects_proof_registered_to_another_spec(self) -> None:
        source = """
proof fn first_proof()
    ensures kernel::first_spec()
{}
proof fn second_proof()
    ensures kernel::second_spec()
{}
"""
        with self.assertRaisesRegex(ValueError, "does not reference registered spec"):
            validate_proof_binding(source, "shared-expression", "first", "second_proof")

    def test_rejects_executable_kernel_call_in_another_proof(self) -> None:
        source = """
fn registered_proof() -> (result: int)
    ensures result == 0
{ 0 }
fn unrelated_proof() -> (result: int)
    ensures result == 0
{ kernel::registered_kernel() }
"""
        with self.assertRaisesRegex(ValueError, "does not return every registered kernel call"):
            validate_proof_binding(
                source, "executable", "registered_kernel", "registered_proof"
            )

    def test_accepts_proof_bound_to_registered_spec(self) -> None:
        source = """
proof fn registered_proof()
    ensures kernel::registered_kernel_spec()
{}
"""
        validate_proof_binding(
            source, "shared-expression", "registered_kernel", "registered_proof"
        )

    def test_rejects_spec_reference_only_in_proof_body(self) -> None:
        source = """
proof fn registered_proof()
    ensures true
{
    let ignored = kernel::registered_kernel_spec();
}
"""
        with self.assertRaisesRegex(ValueError, "ensures does not reference"):
            validate_proof_binding(
                source, "shared-expression", "registered_kernel", "registered_proof"
            )

    def test_rejects_ignored_executable_kernel_result(self) -> None:
        source = """
fn registered_proof() -> (result: int)
    ensures result == 0
{
    let ignored = kernel::registered_kernel();
    0
}
"""
        with self.assertRaisesRegex(ValueError, "does not return every registered kernel call"):
            validate_proof_binding(
                source, "executable", "registered_kernel", "registered_proof"
            )

    def test_rejects_unconstrained_executable_result(self) -> None:
        source = """
fn registered_proof() -> (result: int)
    ensures true
{
    kernel::registered_kernel()
}
"""
        with self.assertRaisesRegex(ValueError, "result is not constrained"):
            validate_proof_binding(
                source, "executable", "registered_kernel", "registered_proof"
            )

    def test_accepts_constrained_executable_tail_call(self) -> None:
        source = """
fn registered_proof() -> (result: int)
    ensures result == 0
{
    kernel::registered_kernel()
}
"""
        validate_proof_binding(
            source, "executable", "registered_kernel", "registered_proof"
        )

    def test_rejects_non_proof_function_for_spec_obligation(self) -> None:
        source = """
fn registered_proof()
    ensures kernel::registered_kernel_spec()
{}
"""
        with self.assertRaisesRegex(ValueError, "resolve exactly once"):
            validate_proof_binding(
                source, "shared-expression", "registered_kernel", "registered_proof"
            )

    def test_rejects_call_site_outside_rust_production_roots(self) -> None:
        with self.assertRaisesRegex(ValueError, "outside Rust production roots"):
            production_call_site_path("scripts/test_verus_manifest.py")


if __name__ == "__main__":
    unittest.main()
