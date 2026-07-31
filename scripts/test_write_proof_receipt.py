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
    def test_receipt_uses_fresh_claim_evidence_without_a_cached_report(self) -> None:
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
            stages.write_text(
                "".join(f"{stage}\tpass\n" for stage in write_proof_receipt.REQUIRED),
                encoding="utf-8",
            )
            with (
                patch.object(
                    write_proof_receipt,
                    "build_claim_report",
                    return_value={"schema": 1, "claims": [claim]},
                ) as build,
                patch.object(
                    write_proof_receipt,
                    "source_fingerprint",
                    side_effect=[current, current],
                ),
                patch.object(sys, "argv", ["write_proof_receipt.py", str(stages), str(receipt)]),
            ):
                self.assertEqual(write_proof_receipt.main(), 0)
            build.assert_called_once_with()
            document = json.loads(receipt.read_text(encoding="utf-8"))
            self.assertEqual(document["claims"], [claim])
            self.assertEqual(document["source_fingerprint"], current)
            self.assertTrue(document["complete"])

    def test_source_change_during_claim_computation_fails_closed(self) -> None:
        before = fingerprint("a" * 64)
        after = fingerprint("b" * 64)
        with (
            patch.object(
                write_proof_receipt,
                "build_claim_report",
                return_value={"schema": 1, "claims": [{"id": "claim"}]},
            ),
            patch.object(
                write_proof_receipt,
                "source_fingerprint",
                side_effect=[before, after],
            ),
        ):
            with self.assertRaisesRegex(ValueError, "changed while computing"):
                write_proof_receipt.current_claim_evidence()

    def test_empty_claim_evidence_is_rejected(self) -> None:
        current = fingerprint("a" * 64)
        with (
            patch.object(
                write_proof_receipt,
                "build_claim_report",
                return_value={"schema": 1, "claims": []},
            ),
            patch.object(
                write_proof_receipt,
                "source_fingerprint",
                side_effect=[current, current],
            ),
        ):
            with self.assertRaisesRegex(ValueError, "non-empty"):
                write_proof_receipt.current_claim_evidence()


if __name__ == "__main__":
    unittest.main()
