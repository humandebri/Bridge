#!/usr/bin/env python3
"""Regression tests for the claim-manifest Solidity refinement gate."""

import hashlib
import unittest
import subprocess
import tempfile
from pathlib import Path
from types import SimpleNamespace

from claim_manifest import (
    REQUIRED_CLAIM_POLICY,
    REQUIRED_CONDITIONAL_LIVENESS_POLICY,
    REQUIRED_CONDITIONAL_LIVENESS_IDS,
    conditional_liveness_check_source,
    lean_contract_check_source,
    parse_claim_manifest,
    parse_conditional_liveness_manifest,
)
from halmos_obligations import parse_halmos_obligations, validate_trusted_halmos_sources
from verus_manifest import parse_verus_manifest
from check_claim_manifest import (
    abstract_evidence_status,
    missing_scalar_calls,
    require_mandatory_claim_catalog,
    require_exact_claim_coverage,
    require_exact_implementation_basis,
    require_exact_smt_claim_coverage,
    require_unique_smt_obligations,
    required_strength_met,
    solidity_function_body,
    strip_solidity_comments_and_strings,
    uncovered_verus_obligations,
    validate_lean_axiom_output,
)
from smt_obligations import parse_smt_obligations, validate_trusted_smt_sources
from trusted_proof_profiles import select_profile


TRUSTED_PROOF_PROFILE = select_profile().identifier


class ClaimContractTests(unittest.TestCase):
    def manifest(self, contract: str, witness: str) -> str:
        return (
            "schema\t6\t-\t-\t-\t-\t-\n"
            f"contract\tclaim_id\thistory-safety\trelease-safety\tproduction-linked\t{contract}\t{witness}\n"
            "protocol\tclaim_id\tclaim_theorem\t-\ttrace_theorem\t-\t-\t-\t-\t"
            "source.rs#kernel\ttest.rs#case\tassumption\t-\n"
        )

    def test_contract_source_checks_the_expected_type(self) -> None:
        manifest = parse_claim_manifest(
            self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        )
        source = lean_contract_check_source(manifest)
        self.assertIn(
            "example : BridgeSpec.Contract := by", source
        )
        self.assertIn("fail_if_success exact True.intro", source)
        self.assertIn("exact BridgeSpec.witness", source)
        self.assertIn("#print axioms BridgeSpec.witness", source)

    def test_contract_and_witness_cannot_be_declared_independently(self) -> None:
        with self.assertRaisesRegex(ValueError, "must be paired"):
            parse_claim_manifest(self.manifest("BridgeSpec.Contract", "-"))

    def test_release_safety_requires_a_contract_and_witness(self) -> None:
        with self.assertRaisesRegex(ValueError, "requires a Lean contract"):
            parse_claim_manifest(self.manifest("-", "-"))

    def test_every_claim_requires_one_contract_registration(self) -> None:
        document = self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        document = document.replace("contract\tclaim_id", "contract\tother_claim")
        with self.assertRaisesRegex(ValueError, "coverage differs"):
            parse_claim_manifest(document)

    def test_rejects_literal_true_contract(self) -> None:
        with self.assertRaisesRegex(ValueError, "vacuous Lean claim contract"):
            parse_claim_manifest(self.manifest("True", "True.intro"))

    def test_rejects_legacy_claim_schema(self) -> None:
        legacy = self.manifest("BridgeSpec.Contract", "BridgeSpec.witness").replace(
            "schema\t6\t-\t-\t-\t-\t-", "schema\t5\t-\t-\t-"
        )
        with self.assertRaisesRegex(ValueError, "schema 6"):
            parse_claim_manifest(legacy)

    def test_rejects_invalid_assurance_target_and_strength(self) -> None:
        current = self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        with self.assertRaisesRegex(ValueError, "assurance target"):
            parse_claim_manifest(current.replace("release-safety", "release-claim"))
        with self.assertRaisesRegex(ValueError, "required strength"):
            parse_claim_manifest(current.replace("production-linked", "tested"))

    def test_liveness_cannot_reenter_the_release_claim_catalog(self) -> None:
        current = self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        with self.assertRaisesRegex(ValueError, "proof class"):
            parse_claim_manifest(current.replace("history-safety", "liveness"))

    def test_conditional_liveness_catalog_is_exact(self) -> None:
        root = Path(__file__).resolve().parents[1]
        document = (root / "verification" / "conditional-liveness.tsv").read_text(
            encoding="utf-8"
        )
        self.assertEqual(
            set(parse_conditional_liveness_manifest(document)),
            REQUIRED_CONDITIONAL_LIVENESS_IDS,
        )
        rows = document.splitlines()
        with self.assertRaisesRegex(ValueError, "catalog differs"):
            parse_conditional_liveness_manifest("\n".join(rows[:-1]) + "\n")

    def test_conditional_liveness_policy_rejects_theorem_and_assumption_drift(self) -> None:
        root = Path(__file__).resolve().parents[1]
        document = (root / "verification" / "conditional-liveness.tsv").read_text(
            encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "theorem differs"):
            parse_conditional_liveness_manifest(
                document.replace(
                    "BridgeSpec.Liveness.committed_withdrawal_eventually_paid",
                    "Other.committed_withdrawal_eventually_paid",
                    1,
                )
            )
        with self.assertRaisesRegex(ValueError, "assumptions differ"):
            parse_conditional_liveness_manifest(
                document.replace("eventual_keeper_action;", "", 1)
            )

    def test_conditional_liveness_source_checks_exact_type_and_axioms(self) -> None:
        root = Path(__file__).resolve().parents[1]
        properties = parse_conditional_liveness_manifest(
            (root / "verification" / "conditional-liveness.tsv").read_text(
                encoding="utf-8"
            )
        )
        source = conditional_liveness_check_source(properties)
        theorem, proposition, _ = REQUIRED_CONDITIONAL_LIVENESS_POLICY[
            "withdrawal_eventually_paid"
        ]
        self.assertIn(f"example : {proposition} := by", source)
        self.assertIn(f"exact {theorem}", source)
        self.assertIn(f"#print axioms {theorem}", source)

    def test_conditional_liveness_rejects_project_local_axioms(self) -> None:
        with self.assertRaisesRegex(ValueError, "project-local axioms"):
            validate_lean_axiom_output(
                "declaration uses 'sorry'\ndepends on axioms: [BridgeSpec.localAxiom]\n",
                1,
                "conditional liveness theorem",
            )

    def test_release_policy_rejects_missing_mandatory_claims(self) -> None:
        manifest = parse_claim_manifest(
            self.manifest("BridgeSpec.Contract", "BridgeSpec.witness")
        )
        with self.assertRaisesRegex(ValueError, "mandatory claim catalog differs"):
            require_mandatory_claim_catalog(manifest)

    def test_release_policy_rejects_target_and_strength_downgrades(self) -> None:
        root = Path(__file__).resolve().parents[1]
        document = (root / "verification" / "claims.tsv").read_text(encoding="utf-8")
        target_downgrade = document.replace("release-safety", "model-support", 1)
        with self.assertRaisesRegex(ValueError, "mandatory claim policy differs"):
            require_mandatory_claim_catalog(parse_claim_manifest(target_downgrade))

        implementation_claim = next(
            claim_id
            for claim_id, (_, strength) in REQUIRED_CLAIM_POLICY.items()
            if strength == "implementation-proved"
        )
        strength_downgrade = document.replace(
            f"contract\t{implementation_claim}\thistory-safety\trelease-safety\timplementation-proved",
            f"contract\t{implementation_claim}\thistory-safety\trelease-safety\tproduction-linked",
            1,
        )
        with self.assertRaisesRegex(ValueError, "mandatory claim policy differs"):
            require_mandatory_claim_catalog(parse_claim_manifest(strength_downgrade))

    def test_release_policy_rejects_strength_exchange_between_claims(self) -> None:
        root = Path(__file__).resolve().parents[1]
        document = (root / "verification" / "claims.tsv").read_text(encoding="utf-8")
        implementation_claim = next(
            claim_id
            for claim_id, (_, strength) in REQUIRED_CLAIM_POLICY.items()
            if strength == "implementation-proved"
        )
        linked_claim = next(
            claim_id
            for claim_id, (_, strength) in REQUIRED_CLAIM_POLICY.items()
            if strength == "production-linked"
        )
        exchanged = document.replace(
            f"contract\t{implementation_claim}\thistory-safety\trelease-safety\timplementation-proved",
            f"contract\t{implementation_claim}\thistory-safety\trelease-safety\tSWAP",
            1,
        ).replace(
            f"contract\t{linked_claim}\thistory-safety\trelease-safety\tproduction-linked",
            f"contract\t{linked_claim}\thistory-safety\trelease-safety\timplementation-proved",
            1,
        ).replace("\tSWAP\t", "\tproduction-linked\t", 1)
        with self.assertRaisesRegex(ValueError, "mandatory claim policy differs"):
            require_mandatory_claim_catalog(parse_claim_manifest(exchanged))

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

    def test_comments_and_strings_do_not_define_functions_or_calls(self) -> None:
        source = r'''
// function commentOnly() internal { removedKernel(); }
function evaluateMint() internal {
    string memory doubleQuoted = "} removedKernel(); \" {";
    string memory singleQuoted = '} removedKernel(); \' {';
    /* removedKernel(); } */
    deadlineAccepts(0, 0);
}
'''
        body = solidity_function_body(source, "evaluateMint")
        self.assertNotIn("removedKernel", body)
        self.assertIn("deadlineAccepts", body)
        with self.assertRaisesRegex(ValueError, "missing Solidity function"):
            solidity_function_body(source, "commentOnly")

    def test_string_stripping_preserves_offsets_and_newlines(self) -> None:
        source = 'function f() { string memory value = "{\\\"}";\nreturn; }'
        cleaned = strip_solidity_comments_and_strings(source)
        self.assertEqual(len(cleaned), len(source))
        self.assertEqual(cleaned.count("\n"), source.count("\n"))

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

    def test_rejects_unterminated_comment(self) -> None:
        with self.assertRaisesRegex(ValueError, "unterminated Solidity block"):
            strip_solidity_comments_and_strings("function f() {} /*")

    def test_rejects_unterminated_string(self) -> None:
        with self.assertRaisesRegex(ValueError, "unterminated Solidity string"):
            strip_solidity_comments_and_strings('function f() { string memory x = "')


class SmtObligationTests(unittest.TestCase):
    def manifest(self, strength: str = "supporting") -> str:
        return (
            "schema\t2\t-\t-\t-\t-\t-\n"
            "obligation\texample\t"
            f"{strength}\tpass.sol#check\tproduction.sol#kernel\tfailure\tclaim\n"
        )

    def test_parses_typed_obligation(self) -> None:
        obligation = parse_smt_obligations(self.manifest())["example"]
        self.assertEqual(obligation.strength, "supporting")
        self.assertEqual(obligation.pass_links, ("pass.sol#check",))

    def test_rejects_unknown_strength(self) -> None:
        with self.assertRaisesRegex(ValueError, "invalid SMT obligation strength"):
            parse_smt_obligations(self.manifest("implementation-proved"))

    def test_rejects_incomplete_obligation(self) -> None:
        with self.assertRaisesRegex(ValueError, "incomplete SMT obligation"):
            parse_smt_obligations(self.manifest().replace("failure\tclaim", "-\tclaim"))

    def test_rejects_claim_complete_strength(self) -> None:
        with self.assertRaisesRegex(ValueError, "invalid SMT obligation strength"):
            parse_smt_obligations(self.manifest("claim-complete"))

    def test_rejects_obsolete_call_site_column(self) -> None:
        document = self.manifest().replace(
            "production.sol#kernel\tfailure",
            "production.sol#kernel\tcaller.sol#apply\tfailure",
        )
        with self.assertRaisesRegex(ValueError, "invalid SMT obligation row"):
            parse_smt_obligations(document)

    def test_rejects_duplicate_claim_ids(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate SMT claim IDs"):
            parse_smt_obligations(self.manifest().replace("\tclaim\n", "\tclaim;claim\n"))

    def test_trusted_source_digests_reject_harness_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            relative = "verification/smt/pass/Harness.sol"
            harness = root / relative
            harness.parent.mkdir(parents=True)
            harness.write_text("assert(state);\n", encoding="utf-8")
            expected = {relative: hashlib.sha256(harness.read_bytes()).hexdigest()}
            validate_trusted_smt_sources(root, expected)
            harness.write_text("assert(true);\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "source digest differs"):
                validate_trusted_smt_sources(root, expected)


class SmtClaimCoverageTests(unittest.TestCase):
    def test_accepts_exact_bidirectional_coverage(self) -> None:
        require_exact_smt_claim_coverage(
            {"obligation": {"claim_a", "claim_b"}},
            {"obligation": {"claim_a", "claim_b"}},
        )

    def test_rejects_claim_declared_only_by_obligation(self) -> None:
        with self.assertRaisesRegex(ValueError, "claim coverage differs"):
            require_exact_smt_claim_coverage(
                {"obligation": {"claim_a", "claim_b"}},
                {"obligation": {"claim_a"}},
            )

    def test_rejects_claim_referenced_only_by_claim_manifest(self) -> None:
        with self.assertRaisesRegex(ValueError, "claim coverage differs"):
            require_exact_smt_claim_coverage(
                {"obligation": {"claim_a"}},
                {"obligation": {"claim_a", "claim_b"}},
            )

    def test_rejects_duplicate_obligation_references_in_one_claim(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate SMT obligations"):
            require_unique_smt_obligations(
                "claim_a", ["obligation", "obligation"]
            )


class VerusClaimCoverageTests(unittest.TestCase):
    def test_accepts_exact_bidirectional_coverage(self) -> None:
        require_exact_claim_coverage(
            "Verus",
            {"proof": {"claim_a", "claim_b"}},
            {"proof": {"claim_a", "claim_b"}},
        )

    def test_rejects_claim_omitted_from_claim_manifest(self) -> None:
        with self.assertRaisesRegex(ValueError, "Verus obligation claim coverage differs"):
            require_exact_claim_coverage(
                "Verus",
                {"proof": {"claim_a", "claim_b"}},
                {"proof": {"claim_a"}},
            )

    def test_rejects_claim_added_only_to_claim_manifest(self) -> None:
        with self.assertRaisesRegex(ValueError, "Verus obligation claim coverage differs"):
            require_exact_claim_coverage(
                "Verus",
                {"proof": {"claim_a"}},
                {"proof": {"claim_a", "claim_b"}},
            )


class ImplementationBasisTests(unittest.TestCase):
    @staticmethod
    def verus_rows() -> dict[str, list[SimpleNamespace]]:
        return {
            "proof_a": [SimpleNamespace(production_bound=True)],
            "proof_b": [SimpleNamespace(production_bound=True)],
            "model_only": [SimpleNamespace(production_bound=False)],
        }

    def test_accepts_exact_all_of_basis(self) -> None:
        required = require_exact_implementation_basis(
            "claim",
            ["verus:proof_b", "verus:proof_a"],
            ["proof_a", "proof_b"],
            [],
            self.verus_rows(),
            {},
        )
        self.assertEqual(required, {"verus:proof_a", "verus:proof_b"})

    def test_rejects_missing_required_basis(self) -> None:
        with self.assertRaisesRegex(ValueError, "implementation basis differs"):
            require_exact_implementation_basis(
                "claim",
                ["verus:proof_a"],
                ["proof_a", "proof_b"],
                [],
                self.verus_rows(),
                {},
            )

    def test_rejects_unbound_extra_basis(self) -> None:
        with self.assertRaisesRegex(ValueError, "implementation basis differs"):
            require_exact_implementation_basis(
                "claim",
                ["verus:model_only"],
                ["model_only"],
                [],
                self.verus_rows(),
                {},
            )

    def test_rejects_nonempty_basis_when_no_evidence_is_required(self) -> None:
        with self.assertRaisesRegex(ValueError, "implementation basis differs"):
            require_exact_implementation_basis(
                "claim",
                ["verus:proof_a"],
                [],
                [],
                self.verus_rows(),
                {},
            )

    def test_halmos_supporting_evidence_cannot_be_implementation_basis(self) -> None:
        with self.assertRaisesRegex(ValueError, "implementation basis differs"):
            require_exact_implementation_basis(
                "claim",
                ["halmos:complete"],
                [],
                ["complete", "supporting"],
                {},
                {
                    "complete": SimpleNamespace(claim_complete=True),
                    "supporting": SimpleNamespace(claim_complete=False),
                },
            )

    def test_required_strength_order_fails_closed(self) -> None:
        self.assertTrue(required_strength_met("implementation-proved", "production-linked"))
        self.assertFalse(required_strength_met("production-linked", "implementation-proved"))


class VerusImplementationCoverageTests(unittest.TestCase):
    @staticmethod
    def obligation(
        proof: str,
        kind: str,
        *,
        production_bound: bool = False,
        kernel: str | None = None,
        binding: tuple[str, ...] = (),
        claims: tuple[str, ...] = ("claim",),
    ) -> SimpleNamespace:
        return SimpleNamespace(
            proof=proof,
            kind=kind,
            production_bound=production_bound,
            kernel=kernel or proof,
            binding=binding,
            claim_ids=claims,
        )

    def test_one_bound_obligation_does_not_cover_an_unbound_sibling(self) -> None:
        rows = {
            "bound": [self.obligation("bound", "executable", production_bound=True)],
            "unbound": [self.obligation("unbound", "shared-expression")],
        }
        self.assertEqual(
            uncovered_verus_obligations("claim", ["bound", "unbound"], rows),
            {"unbound"},
        )

    def test_model_obligation_is_not_implementation_coverage(self) -> None:
        rows = {"model": [self.obligation("model", "model")]}
        self.assertEqual(
            uncovered_verus_obligations("claim", ["model"], rows), {"model"}
        )

    def test_derived_is_covered_by_same_claim_bound_dependencies(self) -> None:
        dependency = self.obligation(
            "base_proof", "shared-expression", production_bound=True, kernel="base"
        )
        derived = self.obligation(
            "derived_proof", "derived", binding=("base",)
        )
        rows = {"base_proof": [dependency], "derived_proof": [derived]}
        self.assertEqual(
            uncovered_verus_obligations(
                "claim", ["base_proof", "derived_proof"], rows
            ),
            set(),
        )

    def test_derived_dependency_must_be_bound_and_registered_for_same_claim(self) -> None:
        dependency = self.obligation(
            "base_proof",
            "shared-expression",
            production_bound=True,
            kernel="base",
            claims=("other_claim",),
        )
        derived = self.obligation(
            "derived_proof", "derived", binding=("base",)
        )
        rows = {"base_proof": [dependency], "derived_proof": [derived]}
        self.assertEqual(
            uncovered_verus_obligations("claim", ["derived_proof"], rows),
            {"derived_proof"},
        )

    @unittest.skipUnless(
        TRUSTED_PROOF_PROFILE in {"current-main", "security-hardening-v1"},
        "requires the current hardening claim schema",
    )
    def test_current_mixed_strength_claims_are_not_implementation_covered(self) -> None:
        root = Path(__file__).resolve().parents[1]
        claims = parse_claim_manifest(
            (root / "verification" / "claims.tsv").read_text(encoding="utf-8")
        )
        registrations = parse_verus_manifest(
            (root / "verification" / "verus" / "manifest.tsv").read_text(
                encoding="utf-8"
            )
        )
        rows: dict[str, list[object]] = {}
        for registration in registrations.values():
            rows.setdefault(registration.proof, []).append(registration)
        actual = {
            claim_id
            for (
                _,
                claim_id,
                _,
                _,
                _,
                verus_proofs,
                _,
                _,
                implementation_basis,
                _,
                _,
                _,
                _,
            ) in claims.rows
            if implementation_basis != "-"
            and uncovered_verus_obligations(
                claim_id,
                [] if verus_proofs == "-" else verus_proofs.split(";"),
                rows,
            )
        }
        self.assertEqual(
            actual,
            {
                "settlement_backing",
                "payment_identity",
                "deposit_admission",
                "fee_payout",
                "hold_resolution",
                "lease_outcome",
                "expiry_refund",
                "reservation_lifecycle",
                "fee_accounting_once",
                "deposit_backing",
                "refund_evidence_enforcement",
            },
        )


class HalmosObligationTests(unittest.TestCase):
    @staticmethod
    def manifest(strength: str = "supporting") -> str:
        return (
            "schema\t1\t-\t-\t-\t-\t-\n"
            "obligation\texample\t"
            f"{strength}\tpass.t.sol#check\tBridge.sol#commit\tfailure\tclaim\n"
        )

    def test_parses_supporting_obligation(self) -> None:
        obligation = parse_halmos_obligations(self.manifest())["example"]
        self.assertEqual(obligation.strength, "supporting")
        self.assertFalse(obligation.claim_complete)

    def test_rejects_claim_complete_strength(self) -> None:
        with self.assertRaisesRegex(ValueError, "invalid Halmos obligation strength"):
            parse_halmos_obligations(self.manifest("claim-complete"))

    def test_rejects_duplicate_claim_ids(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate Halmos claim IDs"):
            parse_halmos_obligations(
                self.manifest().replace("\tclaim\n", "\tclaim;claim\n")
            )

    def test_trusted_source_digests_reject_harness_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            relative = "contracts/test/halmos/Harness.t.sol"
            harness = root / relative
            harness.parent.mkdir(parents=True)
            harness.write_text("assert(state);\n", encoding="utf-8")
            expected = {relative: hashlib.sha256(harness.read_bytes()).hexdigest()}
            validate_trusted_halmos_sources(root, expected)
            harness.write_text("assert(true);\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "source digest differs"):
                validate_trusted_halmos_sources(root, expected)


if __name__ == "__main__":
    unittest.main()
