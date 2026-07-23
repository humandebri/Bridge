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
    "finalization_cases",
    "queue_cases",
    "fee_guard_pending_cases",
    "canonical_probe_cases",
)
DOCUMENT = {"schema_version": 2, **{section: [{}] for section in SECTIONS}}
MODEL = """
def commit := True
def outboundSettlement := True
def decideWithdrawalFinalization := True
def restorePendingQueue := True
def handleNotificationFailure := True
def canonicalProbeMatches := True
"""
THEOREMS = """
theorem committed_quote_is_fixed (h : commit) : True := by trivial
theorem outbound_settlement_preserves_backing (h : outboundSettlement) : True := by trivial
theorem paid_debt_preserves_backing (h : settleDebt) : True := by trivial
theorem withdrawal_notify_requires_finalized_success
    (h : decideWithdrawalFinalization) : True := by trivial
theorem restore_preserves_blocked_retry (h : restorePendingQueue) : True := by trivial
theorem fee_guard_failure_retains_pending
    (h : handleNotificationFailure) : True := by trivial
theorem canonical_probe_matches_exactly
    (h : canonicalProbeMatches) : True := by trivial
"""
VALID_ROWS = [
    "quote_cases\tcommit\tcommitted_quote_is_fixed\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_quote_cases_matches_production",
    "quote_cases\tcommit\tcommitted_quote_is_fixed\tfoundry\t"
    "contracts/test/ProtocolVectors.t.sol\ttest_protocol_quote_cases_matches_production",
    "settlement_cases\toutboundSettlement\toutbound_settlement_preserves_backing\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_settlement_cases_matches_production",
    "finalization_cases\tdecideWithdrawalFinalization\twithdrawal_notify_requires_finalized_success\tvitest\t"
    "ui/src/lib/protocol-vectors.test.ts\tprotocol_finalization_cases_matches_production",
    "queue_cases\trestorePendingQueue\trestore_preserves_blocked_retry\tvitest\t"
    "ui/src/lib/protocol-vectors.test.ts\tprotocol_queue_cases_matches_production",
    "fee_guard_pending_cases\thandleNotificationFailure\tfee_guard_failure_retains_pending\tvitest\t"
    "ui/src/lib/protocol-vectors.test.ts\tprotocol_fee_guard_pending_cases_matches_production",
    "canonical_probe_cases\tcanonicalProbeMatches\tcanonical_probe_matches_exactly\trust\t"
    "canister/bridge-core/tests/protocol_vectors.rs\tprotocol_canonical_probe_cases_matches_production",
]


class RefinementManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        for _, target in refinement.RUNNER_TARGETS:
            path = self.root / target
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("consumer fixture\n", encoding="utf-8")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def parse(self, rows: list[str] | None = None, theorems: str = THEOREMS):
        return refinement.parse_manifest(
            DOCUMENT,
            "\n".join(VALID_ROWS if rows is None else rows) + "\n",
            MODEL,
            theorems,
            self.root,
        )

    def assert_invalid(self, rows: list[str], message: str, theorems: str = THEOREMS) -> None:
        with self.assertRaisesRegex(ValueError, message):
            self.parse(rows, theorems)

    def test_valid_manifest_registers_every_consumer(self) -> None:
        consumers = self.parse()
        self.assertEqual(len(consumers), 7)
        self.assertEqual({consumer.section for consumer in consumers}, set(SECTIONS))

    def test_old_settlement_theorem_is_rejected(self) -> None:
        rows = [
            row.replace("outbound_settlement_preserves_backing", "paid_debt_preserves_backing")
            if row.startswith("settlement_cases\t") else row
            for row in VALID_ROWS
        ]
        self.assert_invalid(rows, "does not directly reference outboundSettlement")

    def test_conflicting_association_is_rejected(self) -> None:
        rows = VALID_ROWS.copy()
        rows[1] = rows[1].replace("committed_quote_is_fixed", "paid_debt_preserves_backing")
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
        rows = [row for row in VALID_ROWS if not row.startswith("queue_cases\t")]
        self.assert_invalid(rows, "do not match vectors")


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
