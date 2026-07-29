#!/usr/bin/env python3
"""Regression tests for logic-to-proof impact enforcement."""

import json
import tempfile
import unittest
from pathlib import Path

import check_proof_impact


ROOT = Path(__file__).resolve().parents[1]


class ProofImpactTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.manifest = check_proof_impact.load_manifest()

    def test_every_current_safety_source_is_registered(self) -> None:
        registered = {
            source for area in self.manifest.areas for source in area.sources
        }
        for watched in self.manifest.roots:
            for path in (ROOT / watched.path).rglob(f"*{watched.suffix}"):
                with self.subTest(path=path):
                    self.assertIn(path.relative_to(ROOT).as_posix(), registered)

    def test_deposit_kernel_routes_to_all_claims_and_stages(self) -> None:
        impact = check_proof_impact.classify_paths(
            ["canister/bridge-core/src/kernel.rs"], self.manifest
        )
        for claim in (
            "authorization_binding",
            "expiry_refund",
            "exact_mint_finalization",
            "reservation_lifecycle",
            "fee_accounting_once",
            "deposit_backing",
        ):
            self.assertIn(claim, impact["claims"])
        self.assertEqual(impact["stages"], list(check_proof_impact.REQUIRED_STAGES))

    def test_solidity_policy_routes_to_mint_claims(self) -> None:
        impact = check_proof_impact.classify_paths(
            ["contracts/src/libraries/MintAuthorizationPolicy.sol"], self.manifest
        )
        self.assertIn("authorization_binding", impact["claims"])
        self.assertIn("epoch_invalidation", impact["claims"])
        self.assertNotIn("payment_identity", impact["claims"])

    def test_new_safety_source_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "unregistered"):
            check_proof_impact.classify_paths(
                ["canister/bridge-core/src/new_policy.rs"], self.manifest
            )

    def test_documentation_has_no_proof_impact(self) -> None:
        impact = check_proof_impact.classify_paths(
            ["docs/bridge-flow.md"], self.manifest
        )
        self.assertEqual(impact["areas"], [])
        self.assertEqual(impact["claims"], [])
        self.assertEqual(impact["stages"], [])

    def test_receipt_rejects_stale_fingerprint(self) -> None:
        receipt = {
            "required_stages": list(check_proof_impact.REQUIRED_STAGES),
            "source_fingerprint": {
                "algorithm": "sha256",
                "digest": "stale",
                "input_count": 0,
            },
            "complete": True,
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "stale"):
                check_proof_impact.check_receipt(path)


if __name__ == "__main__":
    unittest.main()
