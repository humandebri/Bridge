#!/usr/bin/env python3
"""Regression tests for refinement manifest structure and consumer attestations."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import check_refinement_manifest as refinement


SECTIONS = (
    "quote_cases",
    "settlement_cases",
)
DOCUMENT = {"schema_version": 2, **{section: [{}] for section in SECTIONS}}
MODEL = """
def commit := True
def checkedSettlement := True
"""
IMPLEMENTATIONS = """
def commitImpl := True
def settlementImpl := True
"""
REFINEMENTS = """
theorem committed_quote_refinement (h : commit) (i : commitImpl) : True := by trivial
theorem settlement_backing_refinement (h : checkedSettlement) (i : settlementImpl) : True := by trivial
"""
CLAIMS = """
theorem committed_quote_claim : True := by trivial
theorem settlement_backing_claim : True := by trivial
"""
PROTOCOL = """
def rawStep := True
def step := rawStep
theorem raw_step_preserves_safe (safe : Safe state)
    (accepted : rawStep state event = some next) : Safe next := by trivial
theorem step_preserves_safe : True := by
  exact raw_step_preserves_safe safe accepted
theorem conditional_committed_withdrawal_reaches_paid
    (canonicalValid : True) (cycles : True) (fair : True) : True := by
  let events := [.observeCanonical, .executorClaim, .settle]
  trivial
theorem committed_quote_trace : True := by trivial
theorem settlement_backing_trace : True := by trivial
"""
VALID_ROWS = [
    "quote_cases\tcommit\tcommitImpl\tcommitted_quote_refinement\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_quote_cases_matches_production",
    "quote_cases\tcommit\tcommitImpl\tcommitted_quote_refinement\tfoundry\t"
    "contracts/test/ProtocolVectors.t.sol\ttest_protocol_quote_cases_matches_production",
    "settlement_cases\tcheckedSettlement\tsettlementImpl\tsettlement_backing_refinement\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_settlement_cases_matches_production",
]
ASSUMPTIONS = (
    "runtime_toolchain\ttool semantics\tcommitted_quote;settlement_backing\t"
    "canister/bridge-core/tests/protocol_vectors.rs#protocol_quote_cases_matches_production\t"
    "proof gate monitoring\tabort before mutation\n"
)
VERUS_MANIFEST = (
    "shared\tcommit_exec\tcommit_exec_proof\tcommit.rs\tfixture.rs\n"
    "executable\tsettlement_exec\tsettlement_exec_proof\tsettlement.rs\tfixture.rs\n"
)
PROOF_LINKS = """
production_link!(
    "committed_quote",
    "canister/bridge-core/tests/protocol_vectors.rs#committed_quote_matches",
    commit,
    fn()
);
production_link!(
    "settlement_backing",
    "canister/bridge-core/tests/protocol_vectors.rs#checked_settlement",
    settlement,
    fn()
);
"""
CLAIM_ROWS = [
    "committed_quote\tcommitted_quote_claim\tcommitted_quote_refinement\tcommitted_quote_trace\t"
    "commit_exec_proof\tquote_cases\t"
    "canister/bridge-core/tests/protocol_vectors.rs#committed_quote_matches\t"
    "canister/bridge-core/tests/protocol_vectors.rs#protocol_quote_cases_matches_production\t"
    "runtime_toolchain\tproved\tproved\tproved\tproved\trefinement-tested\tassumed\t"
    "phase2\tcomplete",
    "settlement_backing\tsettlement_backing_claim\tsettlement_backing_refinement\t"
    "raw_step_preserves_safe;step_preserves_safe;conditional_committed_withdrawal_reaches_paid;"
    "settlement_backing_trace\t"
    "settlement_exec_proof\tsettlement_cases\t"
    "canister/bridge-core/tests/protocol_vectors.rs#checked_settlement\t"
    "canister/bridge-core/tests/protocol_vectors.rs#protocol_settlement_cases_matches_production\t"
    "runtime_toolchain\tproved\tproved\tproved\texecutable-proved\trefinement-tested\t"
    "assumed\tphase2\tcomplete",
]


class RefinementManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        for _, target in refinement.RUNNER_TARGETS:
            path = self.root / target
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                "consumer fixture committed_quote_matches checked_settlement "
                "protocol_quote_cases_matches_production "
                "protocol_settlement_cases_matches_production\n",
                encoding="utf-8",
            )
        verus_pass = self.root / "verification/verus/pass.rs"
        verus_pass.parent.mkdir(parents=True, exist_ok=True)
        verus_pass.write_text(
            "fn settlement_exec_proof() { kernel::settlement_exec(); }\n"
            "proof fn commit_exec_proof() {}\n",
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def parse(
        self,
        rows: list[str] | None = None,
        refinements: str = REFINEMENTS,
        claims: str = CLAIMS,
        protocol: str = PROTOCOL,
        claim_rows: list[str] | None = None,
        assumptions: str = ASSUMPTIONS,
        proof_links: str = PROOF_LINKS,
    ):
        return refinement.parse_manifest(
            DOCUMENT,
            "\n".join(VALID_ROWS if rows is None else rows) + "\n",
            MODEL,
            IMPLEMENTATIONS,
            refinements,
            claims,
            protocol,
            "\n".join(CLAIM_ROWS if claim_rows is None else claim_rows) + "\n",
            assumptions,
            VERUS_MANIFEST,
            proof_links,
            self.root,
        )

    def assert_invalid(self, rows: list[str], message: str, **kwargs) -> None:
        with self.assertRaisesRegex(ValueError, message):
            self.parse(rows, **kwargs)

    def test_valid_manifest_registers_every_consumer(self) -> None:
        consumers = self.parse()
        self.assertEqual(len(consumers), 3)
        self.assertEqual({consumer.section for consumer in consumers}, set(SECTIONS))

    def test_missing_typed_production_link_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "typed production links"):
            self.parse(proof_links=PROOF_LINKS.split('production_link!(', 1)[0])

    def test_old_settlement_theorem_is_rejected(self) -> None:
        rows = [
            row.replace("settlement_backing_refinement", "paid_debt_preserves_backing")
            if row.startswith("settlement_cases\t") else row
            for row in VALID_ROWS
        ]
        refinements = (
            REFINEMENTS
            + "\ntheorem paid_debt_preserves_backing (h : checkedSettlement) "
            "(i : settlementImpl) : True := by trivial\n"
        )
        self.assert_invalid(
            rows, "do not match Refinement.lean", refinements=refinements
        )

    def test_conflicting_association_is_rejected(self) -> None:
        rows = VALID_ROWS.copy()
        rows[1] = rows[1].replace(
            "committed_quote_refinement", "settlement_backing_refinement"
        )
        self.assert_invalid(rows, "conflicting refinement association")

    def test_duplicate_consumer_is_rejected(self) -> None:
        self.assert_invalid(VALID_ROWS + [VALID_ROWS[0]], "duplicate refinement consumer")

    def test_unknown_runner_is_rejected(self) -> None:
        rows = VALID_ROWS.copy()
        rows[0] = rows[0].replace("\trust\t", "\tshell\t")
        self.assert_invalid(rows, "unsupported refinement runner target")

    def test_repository_escape_is_rejected(self) -> None:
        rows = VALID_ROWS.copy()
        rows[0] = rows[0].replace(
            "canister/bridge-core/tests/protocol_vectors.rs", "../protocol_vectors.rs"
        )
        self.assert_invalid(rows, "must stay inside the repository")

    def test_missing_section_is_rejected(self) -> None:
        rows = [row for row in VALID_ROWS if not row.startswith("settlement_cases\t")]
        self.assert_invalid(rows, "do not match vectors")

    def test_unregistered_abstract_theorem_is_rejected(self) -> None:
        claims = CLAIMS + "\ntheorem omitted_claim : True := by trivial\n"
        self.assert_invalid(
            VALID_ROWS,
            "do not match Claims.lean",
            claims=claims,
        )

    def test_unregistered_trace_theorem_is_rejected(self) -> None:
        protocol = PROTOCOL + "\ntheorem omitted_trace : True := by trivial\n"
        with self.assertRaisesRegex(ValueError, "do not match Protocol.lean"):
            self.parse(protocol=protocol)

    def test_safe_post_filter_is_rejected(self) -> None:
        protocol = PROTOCOL.replace("def step := rawStep", "def step := if Safe then rawStep else none")
        with self.assertRaisesRegex(ValueError, "must not filter"):
            self.parse(protocol=protocol)

    def test_raw_preservation_that_does_not_reference_raw_step_is_rejected(self) -> None:
        protocol = PROTOCOL.replace(
            "rawStep state event = some next",
            "step state event = some next",
        )
        with self.assertRaisesRegex(ValueError, "rawStep preservation directly"):
            self.parse(protocol=protocol)

    def test_executable_verus_obligation_cannot_be_replaced_by_spec_proof(self) -> None:
        pass_path = self.root / "verification/verus/pass.rs"
        pass_path.write_text(
            "fn settlement_exec_proof() { kernel::settlement_exec_spec(); }\n"
            "proof fn commit_exec_proof() {}\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "registered production function"):
            self.parse()

    def test_missing_claim_section_is_rejected(self) -> None:
        self.assert_invalid(
            VALID_ROWS,
            "claim Verus obligations|do not match Claims.lean|do not match vectors",
            claim_rows=CLAIM_ROWS[:1],
        )

    def test_missing_production_symbol_is_rejected(self) -> None:
        rows = [
            row.replace("#checked_settlement", "#missing_symbol")
            if row.startswith("settlement_backing\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "missing production symbol",
            claim_rows=rows,
        )

    def test_unknown_assumption_is_rejected(self) -> None:
        rows = [
            row.replace("runtime_toolchain", "unknown_runtime")
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "unknown external assumption",
            claim_rows=rows,
        )

    def test_unknown_verus_obligation_is_rejected(self) -> None:
        rows = [
            row.replace("commit_exec_proof", "missing_exec_proof")
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "unknown Verus obligation",
            claim_rows=rows,
        )

    def test_missing_transaction_test_is_rejected(self) -> None:
        rows = [
            row.replace(
                "#protocol_settlement_cases_matches_production",
                "#missing_transaction_test",
            )
            if row.startswith("settlement_backing\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "missing transaction test",
            claim_rows=rows,
        )

    def test_incomplete_claim_is_rejected(self) -> None:
        rows = [
            row.removesuffix("complete") + "pending"
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "incomplete Phase 5 claim",
            claim_rows=rows,
        )

    def test_forged_evidence_strength_is_rejected(self) -> None:
        rows = [
            row.replace(
                "\tproved\tproved\tproved\texecutable-proved\trefinement-tested\tassumed\t",
                "\tproved\tproved\tproved\tproved\trefinement-tested\tassumed\t",
            )
            if row.startswith("settlement_backing\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "claim evidence mismatch",
            claim_rows=rows,
        )

    def test_missing_external_assumption_classification_is_rejected(self) -> None:
        rows = [
            row.replace("\trefinement-tested\tassumed\t", "\trefinement-tested\tproved\t")
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "claim evidence mismatch",
            claim_rows=rows,
        )

    def test_complete_claim_without_trace_theorem_is_rejected(self) -> None:
        rows = [
            row.replace("\tcommitted_quote_trace\t", "\t-\t")
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "complete claim lacks a trace theorem",
            claim_rows=rows,
        )

    def test_complete_claim_without_verus_obligation_is_rejected(self) -> None:
        rows = [
            row.replace("\tcommit_exec_proof\t", "\t-\t")
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "complete claim lacks a Verus obligation",
            claim_rows=rows,
        )

    def test_unknown_phase_gate_is_rejected(self) -> None:
        rows = [
            row.replace("\tphase2\tcomplete", "\tphase9\tcomplete")
            if row.startswith("committed_quote\t")
            else row
            for row in CLAIM_ROWS
        ]
        self.assert_invalid(
            VALID_ROWS,
            "invalid phase gate",
            claim_rows=rows,
        )

    def test_duplicate_claim_is_rejected(self) -> None:
        self.assert_invalid(
            VALID_ROWS,
            "duplicate Phase 5 claim mapping",
            claim_rows=CLAIM_ROWS + [CLAIM_ROWS[0]],
        )


class ConsumerResultTests(unittest.TestCase):
    def consumer(self, runner: str, selector: str = "protocol_section_matches_production") -> refinement.Consumer:
        targets = {kind: target for kind, target in refinement.RUNNER_TARGETS}
        return refinement.Consumer(
            "section_cases", "definition", "implementation", "theorem",
            runner, targets[runner], selector
        )

    def test_rust_requires_one_named_pass(self) -> None:
        consumer = self.consumer("rust")
        refinement.validate_rust(
            consumer,
            "running 1 test\ntest protocol_section_matches_production ... ok\n"
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n",
        )
        for output in (
            "running 0 tests\ntest result: ok. 0 passed; 0 failed;\n",
            "running 1 test\ntest renamed ... ok\ntest result: ok. 1 passed; 0 failed;\n",
            "running 1 test\ntest protocol_section_matches_production ... FAILED\n",
        ):
            with self.subTest(output=output), self.assertRaises(ValueError):
                refinement.validate_rust(consumer, output)

    def test_foundry_requires_one_named_success(self) -> None:
        consumer = self.consumer("foundry")
        valid = {"suite": {"test_results": {f"{consumer.selector}()": {"status": "Success"}}}}
        refinement.validate_foundry(consumer, json.dumps(valid))
        invalid = (
            {},
            {"suite": {"test_results": {f"{consumer.selector}()": {"status": "Skipped"}}}},
            {"suite": {"test_results": {
                f"{consumer.selector}()": {"status": "Success"},
                "extra()": {"status": "Success"},
            }}},
        )
        for report in invalid:
            with self.subTest(report=report), self.assertRaises(ValueError):
                refinement.validate_foundry(consumer, json.dumps(report))

    def test_vitest_requires_one_named_pass(self) -> None:
        consumer = self.consumer("vitest")
        valid = {
            "success": True,
            "numPassedTests": 1,
            "numFailedTests": 0,
            "testResults": [{"assertionResults": [
                {"title": consumer.selector, "status": "passed"},
                {"title": "unrelated", "status": "skipped"},
            ]}],
        }
        refinement.validate_vitest(consumer, json.dumps(valid))
        for status in ("skipped", "failed"):
            report = {
                **valid,
                "success": status != "failed",
                "numPassedTests": 0,
                "numFailedTests": int(status == "failed"),
                "testResults": [{"assertionResults": [{"title": consumer.selector, "status": status}]}],
            }
            with self.subTest(status=status), self.assertRaises(ValueError):
                refinement.validate_vitest(consumer, json.dumps(report))


if __name__ == "__main__":
    unittest.main()
