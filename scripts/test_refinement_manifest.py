#!/usr/bin/env python3
"""Regression tests for the unified typed claim-manifest gate."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_claim_manifest as claims
from test_check_claim_manifest import SolidityFunctionBodyTests


class UnifiedClaimManifestTests(unittest.TestCase):
    def test_live_manifest_validates_without_writing_the_repository_report(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "claim-report.json"
            with patch.object(claims, "REPORT", report):
                self.assertEqual(claims.main(), 0)

            payload = json.loads(report.read_text(encoding="utf-8"))
            self.assertEqual(payload["schema"], 1)
        self.assertEqual(len(payload["claims"]), 25)

    def test_source_link_cannot_escape_the_repository(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            temporary = Path(directory)
            root = temporary / "repository"
            root.mkdir()
            (temporary / "outside.rs").write_text("fn escaped() {}\n", encoding="utf-8")

            with patch.object(claims, "ROOT", root):
                with self.assertRaisesRegex(ValueError, "missing source link target"):
                    claims.checked_link("../outside.rs#escaped")

    def test_source_link_requires_the_registered_symbol(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "source.rs").write_text("fn present() {}\n", encoding="utf-8")

            with patch.object(claims, "ROOT", root):
                with self.assertRaisesRegex(ValueError, "missing registered source symbol"):
                    claims.checked_link("source.rs#missing")


if __name__ == "__main__":
    unittest.main()
