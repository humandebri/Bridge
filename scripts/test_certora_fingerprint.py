#!/usr/bin/env python3
"""Regression tests for the advisory Certora fingerprint boundary."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import certora_fingerprint
import proof_fingerprint


ROOT = Path(__file__).resolve().parents[1]


class CertoraFingerprintTests(unittest.TestCase):
    def test_release_fingerprint_excludes_only_certora_specific_inputs(self) -> None:
        release_inputs = {
            proof_fingerprint.logical_source_path(path, ROOT).as_posix()
            for path in proof_fingerprint.fingerprint_inputs()
        }
        for relative in (
            "scripts/certora_fingerprint.py",
            "scripts/certora_results.py",
            "scripts/check_certora_manifest.py",
            "scripts/run_certora_advisory.sh",
            "verification/certora/specs/Bridge.spec",
            "verification/certora/confs/Bridge.conf",
        ):
            with self.subTest(relative=relative):
                self.assertNotIn(relative, release_inputs)
        for relative in (
            "contracts/src/Bridge.sol",
            "contracts/test/BridgeTimelock.t.sol",
            "scripts/ci-local.sh",
            "scripts/proof_fingerprint.py",
            "verification/claims.tsv",
            "verification/proof-impact.tsv",
        ):
            with self.subTest(relative=relative):
                self.assertIn(relative, release_inputs)

    def test_certora_fingerprint_covers_advisory_and_shared_inputs(self) -> None:
        inputs = {
            path.relative_to(ROOT).as_posix()
            for path in certora_fingerprint.certora_fingerprint_inputs()
        }
        for relative in (
            ".github/workflows/certora-advisory.yml",
            "scripts/certora_fingerprint.py",
            "scripts/certora_results.py",
            "scripts/check_certora_manifest.py",
            "scripts/proof_fingerprint.py",
            "verification/certora/specs/Bridge.spec",
            "verification/certora/confs/Bridge.conf",
            "verification/certora/obligations.tsv",
            "verification/certora/uv.lock",
            "verification/claims.tsv",
            "verification/assumptions.tsv",
            "contracts/src/Bridge.sol",
            "contracts/test/BridgeTimelock.t.sol",
        ):
            with self.subTest(relative=relative):
                self.assertIn(relative, inputs)

    def test_advisory_workflow_watches_every_exact_input(self) -> None:
        workflow = (ROOT / ".github/workflows/certora-advisory.yml").read_text(
            encoding="utf-8"
        )
        for relative in certora_fingerprint.CERTORA_EXACT_INPUTS:
            with self.subTest(relative=relative):
                self.assertIn(f'- "{relative}"', workflow)

    def test_advisory_workflow_installs_ripgrep_before_solc_installer(self) -> None:
        workflow = (ROOT / ".github/workflows/certora-advisory.yml").read_text(
            encoding="utf-8"
        )
        ripgrep_install = workflow.index("sudo apt-get install --yes ripgrep")
        ripgrep_check = workflow.index("command -v rg")
        solc_installer = workflow.index(
            "scripts/install-certora-solc.sh", ripgrep_check
        )
        self.assertLess(ripgrep_install, ripgrep_check)
        self.assertLess(ripgrep_check, solc_installer)

    def test_certora_fingerprint_rejects_drift_and_wrong_scope(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "fingerprint.json"
            value = certora_fingerprint.write_fingerprint(path)
            self.assertEqual(certora_fingerprint.check_fingerprint(path), value)
            malformed = dict(value)
            malformed["scope"] = "release-proof"
            path.write_text(json.dumps(malformed), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "scope"):
                certora_fingerprint.check_fingerprint(path)
            path.write_text(json.dumps(value), encoding="utf-8")
            changed = dict(value)
            changed["digest"] = "0" * 64
            with mock.patch.object(
                certora_fingerprint,
                "certora_source_fingerprint",
                return_value=changed,
            ):
                with self.assertRaisesRegex(ValueError, "changed"):
                    certora_fingerprint.check_fingerprint(path)

    def test_certora_fingerprint_rejects_release_scope_and_extra_fields(self) -> None:
        release = {
            "algorithm": "sha256",
            "digest": "a" * 64,
            "input_count": 1,
        }
        with self.assertRaisesRegex(ValueError, "shape"):
            certora_fingerprint.validate_certora_fingerprint(release)
        with self.assertRaisesRegex(ValueError, "shape"):
            certora_fingerprint.validate_certora_fingerprint(
                {**release, "scope": "certora-advisory-v1", "unexpected": True}
            )


if __name__ == "__main__":
    unittest.main()
