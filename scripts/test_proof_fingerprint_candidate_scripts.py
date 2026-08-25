#!/usr/bin/env python3
"""Regression tests for candidate-script closure in the hardening profile."""

from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

import proof_fingerprint
import source_resolution


class CandidateScriptFingerprintTests(unittest.TestCase):
    def test_generated_verification_directories_are_excluded(self) -> None:
        for fingerprint in (proof_fingerprint,):
            with self.subTest(module=fingerprint.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory) / "root"
                for relative_root, _ in fingerprint.FINGERPRINT_SOURCE_ROOTS:
                    (root / relative_root).mkdir(parents=True)
                for relative in fingerprint.FINGERPRINT_CONFIG_FILES:
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(f"{relative}\n", encoding="utf-8")
                tracked = root / "verification" / "claims.tsv"
                tracked.write_text("claims\n", encoding="utf-8")
                manifest = SimpleNamespace(areas=(SimpleNamespace(sources=()),))
                before = fingerprint.source_fingerprint(root, manifest)

                for relative in (
                    "verification/output/receipt.json",
                    "verification/lean/.lake/build/cache",
                    "verification/smt/out/build-info.json",
                    "verification/smt/cache/solidity-files-cache.json",
                    "verification/halmos/.venv/marker",
                ):
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text("generated\n", encoding="utf-8")

                self.assertEqual(before, fingerprint.source_fingerprint(root, manifest))

    def test_candidate_script_change_and_deletion_are_fail_closed(self) -> None:
        for fingerprint in (proof_fingerprint,):
            with self.subTest(module=fingerprint.__name__), tempfile.TemporaryDirectory() as directory:
                root = Path(directory) / "root"
                candidate_scripts = Path(directory) / "candidate-scripts"
                for relative_root, _ in fingerprint.FINGERPRINT_SOURCE_ROOTS:
                    (root / relative_root).mkdir(parents=True)
                candidate_scripts.mkdir()
                for relative in fingerprint.FINGERPRINT_CONFIG_FILES:
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(f"{relative}\n", encoding="utf-8")
                script = candidate_scripts / "policy.sh"
                script.write_text("before\n", encoding="utf-8")
                workspace_script = root / "scripts" / "policy.sh"
                workspace_script.write_text("before\n", encoding="utf-8")
                manifest = SimpleNamespace(
                    areas=(SimpleNamespace(sources=("scripts/policy.sh",)),)
                )

                with patch.object(source_resolution, "CANDIDATE_SCRIPTS", None):
                    workspace = fingerprint.source_fingerprint(root, manifest)

                with patch.object(
                    source_resolution, "CANDIDATE_SCRIPTS", str(candidate_scripts)
                ):
                    before = fingerprint.source_fingerprint(root, manifest)
                    self.assertEqual(workspace, before)
                    script.write_text("after\n", encoding="utf-8")
                    after = fingerprint.source_fingerprint(root, manifest)
                    self.assertNotEqual(before["digest"], after["digest"])
                    self.assertEqual(before["input_count"], after["input_count"])
                    script.unlink()
                    with self.assertRaisesRegex(
                        ValueError,
                        "missing proof fingerprint inputs: scripts/policy.sh",
                    ):
                        fingerprint.fingerprint_inputs(root, manifest)


if __name__ == "__main__":
    unittest.main()
