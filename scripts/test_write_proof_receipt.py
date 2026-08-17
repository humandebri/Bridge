#!/usr/bin/env python3
"""Regression tests for source-bound proof receipt claim evidence."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import write_proof_receipt


def fingerprint(digest: str) -> dict[str, object]:
    return {"algorithm": "sha256", "digest": digest, "input_count": 1}


class ProofReceiptTests(unittest.TestCase):
    def report(self, current: dict[str, object], claims: list[dict[str, object]]) -> dict[str, object]:
        return {
            "schema": write_proof_receipt.CLAIM_REPORT_SCHEMA,
            "source_fingerprint": current,
            "claims": claims,
        }

    def write_baseline(self, root: Path, current: dict[str, object]) -> Path:
        baseline = root / "baseline.json"
        baseline.write_text(json.dumps(current), encoding="utf-8")
        return baseline

    def stage_rows(self, current: dict[str, object], status: str = "pass") -> str:
        encoded = json.dumps(current, sort_keys=True)
        return "".join(
            f"{stage}\t{status}\t{encoded}\n"
            for stage in write_proof_receipt.REQUIRED
        )

    def test_complete_receipt_uses_matching_claim_report(self) -> None:
        claim = {
            "id": "current_claim",
            "kind": "protocol",
            "status": "partial",
            "evidence": {},
            "unproved_reasons": ["external_assumptions:test"],
        }
        current = fingerprint("a" * 64)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stages = root / "stages.tsv"
            receipt = root / "receipt.json"
            baseline = self.write_baseline(root, current)
            report_path = root / "claim-report.json"
            report = self.report(current, [claim])
            report_path.write_text(json.dumps(report), encoding="utf-8")
            stages.write_text(self.stage_rows(current), encoding="utf-8")
            with (
                patch.object(
                    write_proof_receipt,
                    "build_claim_report",
                    return_value=report,
                ) as build,
                patch.object(write_proof_receipt, "REPORT", report_path),
                patch.object(
                    write_proof_receipt,
                    "source_fingerprint",
                    side_effect=[current, current],
                ),
                patch.object(
                    sys,
                    "argv",
                    ["write_proof_receipt.py", str(stages), str(receipt), str(baseline)],
                ),
            ):
                self.assertEqual(write_proof_receipt.main(), 0)
            build.assert_called_once_with()
            document = json.loads(receipt.read_text(encoding="utf-8"))
            self.assertEqual(document["claims"], [claim])
            self.assertEqual(document["source_fingerprint"], current)
            self.assertTrue(
                all(stage["source_fingerprint"] == current for stage in document["stages"])
            )
            self.assertEqual(
                document["claim_report_schema"], write_proof_receipt.CLAIM_REPORT_SCHEMA
            )
            self.assertTrue(document["complete"])

    def test_missing_report_writes_incomplete_receipt_and_fails(self) -> None:
        current = fingerprint("a" * 64)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            missing = root / "missing.json"
            stages = root / "stages.tsv"
            receipt = root / "receipt.json"
            baseline = self.write_baseline(root, current)
            stages.write_text(self.stage_rows(current), encoding="utf-8")
            with (
                patch.object(write_proof_receipt, "REPORT", missing),
                patch.object(write_proof_receipt, "source_fingerprint", return_value=current),
                patch.object(
                    sys,
                    "argv",
                    ["write_proof_receipt.py", str(stages), str(receipt), str(baseline)],
                ),
            ):
                self.assertEqual(write_proof_receipt.main(), 1)
            document = json.loads(receipt.read_text(encoding="utf-8"))
            self.assertFalse(document["complete"])
            self.assertEqual(document["claims"], [])
            self.assertIn("missing", document["claim_report_error"])

    def test_missing_report_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            missing = Path(directory) / "missing.json"
            with patch.object(write_proof_receipt, "REPORT", missing):
                with self.assertRaisesRegex(ValueError, "missing"):
                    write_proof_receipt.current_claim_evidence(fingerprint("a" * 64))

    def test_stale_report_is_rejected(self) -> None:
        current = fingerprint("a" * 64)
        stale = fingerprint("b" * 64)
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "claim-report.json"
            report_path.write_text(
                json.dumps(self.report(stale, [{"id": "claim"}])), encoding="utf-8"
            )
            with (
                patch.object(write_proof_receipt, "REPORT", report_path),
                patch.object(write_proof_receipt, "source_fingerprint", side_effect=[current, current]),
                patch.object(
                    write_proof_receipt,
                    "build_claim_report",
                    return_value=self.report(current, [{"id": "claim"}]),
                ),
            ):
                with self.assertRaisesRegex(ValueError, "stale"):
                    write_proof_receipt.current_claim_evidence(current)

    def test_schema_mismatch_is_rejected(self) -> None:
        current = fingerprint("a" * 64)
        report = self.report(current, [{"id": "claim"}])
        report["schema"] = 1
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "claim-report.json"
            report_path.write_text(json.dumps(report), encoding="utf-8")
            with (
                patch.object(write_proof_receipt, "REPORT", report_path),
                patch.object(write_proof_receipt, "source_fingerprint", side_effect=[current, current]),
                patch.object(write_proof_receipt, "build_claim_report", return_value=report),
            ):
                with self.assertRaisesRegex(ValueError, "schema"):
                    write_proof_receipt.current_claim_evidence(current)

    def test_modified_claims_are_rejected(self) -> None:
        current = fingerprint("a" * 64)
        expected = self.report(current, [{"id": "claim", "status": "partial"}])
        modified = self.report(current, [{"id": "claim", "status": "implementation-proved"}])
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "claim-report.json"
            report_path.write_text(json.dumps(modified), encoding="utf-8")
            with (
                patch.object(write_proof_receipt, "REPORT", report_path),
                patch.object(write_proof_receipt, "source_fingerprint", side_effect=[current, current]),
                patch.object(write_proof_receipt, "build_claim_report", return_value=expected),
            ):
                with self.assertRaisesRegex(ValueError, "deterministic"):
                    write_proof_receipt.current_claim_evidence(current)

    def test_source_change_during_claim_computation_fails_closed(self) -> None:
        before = fingerprint("a" * 64)
        after = fingerprint("b" * 64)
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "claim-report.json"
            report = self.report(before, [{"id": "claim"}])
            report_path.write_text(json.dumps(report), encoding="utf-8")
            with (
                patch.object(write_proof_receipt, "REPORT", report_path),
                patch.object(write_proof_receipt, "build_claim_report", return_value=report),
                patch.object(write_proof_receipt, "source_fingerprint", side_effect=[before, after]),
            ):
                with self.assertRaisesRegex(ValueError, "changed while computing"):
                    write_proof_receipt.current_claim_evidence(before)

    def test_empty_claim_evidence_is_rejected(self) -> None:
        current = fingerprint("a" * 64)
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "claim-report.json"
            report = self.report(current, [])
            report_path.write_text(json.dumps(report), encoding="utf-8")
            with (
                patch.object(write_proof_receipt, "REPORT", report_path),
                patch.object(write_proof_receipt, "build_claim_report", return_value=report),
                patch.object(write_proof_receipt, "source_fingerprint", side_effect=[current, current]),
            ):
                with self.assertRaisesRegex(ValueError, "non-empty"):
                    write_proof_receipt.current_claim_evidence(current)

    def test_source_change_before_claim_computation_fails_closed(self) -> None:
        before = fingerprint("a" * 64)
        after = fingerprint("b" * 64)
        with patch.object(write_proof_receipt, "source_fingerprint", return_value=after):
            with self.assertRaisesRegex(ValueError, "proof run started"):
                write_proof_receipt.current_claim_evidence(before)

    def test_stage_fingerprint_mismatch_is_rejected(self) -> None:
        current = fingerprint("a" * 64)
        stale = fingerprint("b" * 64)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stages = root / "stages.tsv"
            receipt = root / "receipt.json"
            baseline = self.write_baseline(root, current)
            stages.write_text(self.stage_rows(stale), encoding="utf-8")
            with patch.object(
                sys,
                "argv",
                ["write_proof_receipt.py", str(stages), str(receipt), str(baseline)],
            ):
                with self.assertRaisesRegex(ValueError, "differs from baseline"):
                    write_proof_receipt.main()

    def test_legacy_two_column_stage_rows_are_rejected(self) -> None:
        current = fingerprint("a" * 64)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stages = root / "stages.tsv"
            receipt = root / "receipt.json"
            baseline = self.write_baseline(root, current)
            stages.write_text("lean\tpass\n", encoding="utf-8")
            with patch.object(
                sys,
                "argv",
                ["write_proof_receipt.py", str(stages), str(receipt), str(baseline)],
            ):
                with self.assertRaisesRegex(ValueError, "invalid proof receipt stage"):
                    write_proof_receipt.main()


if __name__ == "__main__":
    unittest.main()
