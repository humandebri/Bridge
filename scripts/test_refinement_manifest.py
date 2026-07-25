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
REFINEMENTS = """
theorem committed_quote_refinement (h : commit) : True := by trivial
theorem settlement_backing_refinement (h : checkedSettlement) : True := by trivial
"""
CLAIMS = """
theorem committed_quote_claim : True := by trivial
theorem settlement_backing_claim : True := by trivial
"""
VALID_ROWS = [
    "quote_cases\tcommit\tcommitted_quote_refinement\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_quote_cases_matches_production",
    "quote_cases\tcommit\tcommitted_quote_refinement\tfoundry\t"
    "contracts/test/ProtocolVectors.t.sol\ttest_protocol_quote_cases_matches_production",
    "settlement_cases\tcheckedSettlement\tsettlement_backing_refinement\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_settlement_cases_matches_production",
]
ASSUMPTIONS = "runtime_toolchain\ttool semantics\n"
CLAIM_ROWS = [
    "committed_quote\tcommitted_quote_claim\tcommitted_quote_refinement\tquote_cases\t"
    "proved,refinement-tested,assumed\t"
    "canister/bridge-core/tests/protocol_vectors.rs#committed_quote_matches\truntime_toolchain",
    "settlement_backing\tsettlement_backing_claim\tsettlement_backing_refinement\t"
    "settlement_cases\tproved,refinement-tested,assumed\t"
    "canister/bridge-core/tests/protocol_vectors.rs#checked_settlement\truntime_toolchain",
]


class RefinementManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        for _, target in refinement.RUNNER_TARGETS:
            path = self.root / target
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(
                "consumer fixture committed_quote_matches checked_settlement\n",
                encoding="utf-8",
            )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def parse(
        self,
        rows: list[str] | None = None,
        refinements: str = REFINEMENTS,
        claims: str = CLAIMS,
        claim_rows: list[str] | None = None,
        assumptions: str = ASSUMPTIONS,
    ):
        return refinement.parse_manifest(
            DOCUMENT,
            "\n".join(VALID_ROWS if rows is None else rows) + "\n",
            MODEL,
            refinements,
            claims,
            "\n".join(CLAIM_ROWS if claim_rows is None else claim_rows) + "\n",
            assumptions,
            self.root,
        )

    def assert_invalid(self, rows: list[str], message: str, **kwargs) -> None:
        with self.assertRaisesRegex(ValueError, message):
            self.parse(rows, **kwargs)

    def test_valid_manifest_registers_every_consumer(self) -> None:
        consumers = self.parse()
        self.assertEqual(len(consumers), 3)
        self.assertEqual({consumer.section for consumer in consumers}, set(SECTIONS))

    def test_old_settlement_theorem_is_rejected(self) -> None:
        rows = [
            row.replace("settlement_backing_refinement", "paid_debt_preserves_backing")
            if row.startswith("settlement_cases\t") else row
            for row in VALID_ROWS
        ]
        refinements = (
            REFINEMENTS
            + "\ntheorem paid_debt_preserves_backing (h : settleDebt) : True := by trivial\n"
        )
        self.assert_invalid(
            rows, "does not directly reference checkedSettlement", refinements=refinements
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

    def test_missing_claim_section_is_rejected(self) -> None:
        self.assert_invalid(
            VALID_ROWS,
            "do not match Claims.lean|do not match vectors",
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

    def test_duplicate_claim_is_rejected(self) -> None:
        self.assert_invalid(
            VALID_ROWS,
            "duplicate Phase 5 claim mapping",
            claim_rows=CLAIM_ROWS + [CLAIM_ROWS[0]],
        )


class ConsumerResultTests(unittest.TestCase):
    def consumer(self, runner: str, selector: str = "protocol_section_matches_production") -> refinement.Consumer:
        targets = {kind: target for kind, target in refinement.RUNNER_TARGETS}
        return refinement.Consumer("section_cases", "definition", "theorem", runner, targets[runner], selector)

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
