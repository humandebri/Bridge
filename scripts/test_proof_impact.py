#!/usr/bin/env python3
"""Regression tests for logic-to-proof impact enforcement."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

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

    def valid_receipt(self) -> dict[str, object]:
        return {
            "schema": check_proof_impact.RECEIPT_SCHEMA,
            "required_stages": list(check_proof_impact.REQUIRED_STAGES),
            "stages": [
                {"id": stage, "status": "pass"}
                for stage in check_proof_impact.REQUIRED_STAGES
            ],
            "source_fingerprint": {"algorithm": "sha256", "digest": "current"},
            "claims": [{"id": "claim"}],
            "complete": True,
        }

    def check_receipt(self, receipt: dict[str, object]) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            with patch.object(
                check_proof_impact,
                "source_fingerprint",
                return_value={"algorithm": "sha256", "digest": "current"},
            ):
                check_proof_impact.check_receipt(path)

    def test_receipt_accepts_complete_recomputed_contents(self) -> None:
        self.check_receipt(self.valid_receipt())

    def test_receipt_rejects_missing_duplicate_unknown_and_failed_stages(self) -> None:
        mutations = {
            "missing": lambda stages: stages.pop(),
            "duplicate": lambda stages: stages.__setitem__(1, stages[0]),
            "unknown": lambda stages: stages[0].__setitem__("id", "unknown"),
            "failed": lambda stages: stages[0].__setitem__("status", "fail"),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                receipt = self.valid_receipt()
                mutate(receipt["stages"])
                with self.assertRaisesRegex(ValueError, "stage"):
                    self.check_receipt(receipt)

    def test_receipt_rejects_empty_claims_and_forged_complete_flag(self) -> None:
        empty_claims = self.valid_receipt()
        empty_claims["claims"] = []
        with self.assertRaisesRegex(ValueError, "claims"):
            self.check_receipt(empty_claims)

        forged = self.valid_receipt()
        forged["complete"] = False
        with self.assertRaisesRegex(ValueError, "completion flag"):
            self.check_receipt(forged)

    def test_receipt_rejects_wrong_schema_and_untyped_stages(self) -> None:
        for schema in (2, 3.0, True):
            with self.subTest(schema=schema):
                wrong_schema = self.valid_receipt()
                wrong_schema["schema"] = schema
                with self.assertRaisesRegex(ValueError, "schema"):
                    self.check_receipt(wrong_schema)

        untyped = self.valid_receipt()
        untyped["stages"] = ["lean"]
        with self.assertRaisesRegex(ValueError, "typed records"):
            self.check_receipt(untyped)

    def test_receipt_rejects_stale_fingerprint(self) -> None:
        receipt = self.valid_receipt()
        receipt["source_fingerprint"] = {
            "algorithm": "sha256",
            "digest": "stale",
            "input_count": 0,
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "stale"):
                check_proof_impact.check_receipt(path)

    def test_conservative_fingerprint_covers_consumers_drivers_and_configs(self) -> None:
        inputs = {
            path.relative_to(ROOT).as_posix()
            for path in check_proof_impact.fingerprint_inputs()
        }
        for relative in (
            "contracts/test/ProtocolVectors.t.sol",
            "ui/src/lib/mint-authorization.ts",
            "ui/src/lib/mint-authorization.test.ts",
            "scripts/check_tool_versions.sh",
            "scripts/plan007/evm-rpc-fault-injector",
            "contracts/foundry.toml",
            "Cargo.lock",
            "pnpm-lock.yaml",
            "ui/vitest.config.ts",
        ):
            with self.subTest(relative=relative):
                self.assertIn(relative, inputs)

    def test_fingerprint_changes_when_a_consumer_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative_root, _ in check_proof_impact.FINGERPRINT_SOURCE_ROOTS:
                (root / relative_root).mkdir(parents=True)
            for relative in check_proof_impact.FINGERPRINT_CONFIG_FILES:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"{relative}\n", encoding="utf-8")
            verification = root / "verification"
            verification.mkdir()
            (verification / "claims.tsv").write_text("claims\n", encoding="utf-8")
            consumer = root / "contracts" / "test" / "ProtocolVectors.t.sol"
            consumer.parent.mkdir(parents=True)
            consumer.write_text("contract Consumer {}\n", encoding="utf-8")
            manifest = check_proof_impact.ImpactManifest((), ())

            before = check_proof_impact.source_fingerprint(root, manifest)
            consumer.write_text("contract ChangedConsumer {}\n", encoding="utf-8")
            after = check_proof_impact.source_fingerprint(root, manifest)

            self.assertNotEqual(before["digest"], after["digest"])


if __name__ == "__main__":
    unittest.main()
