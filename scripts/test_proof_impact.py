#!/usr/bin/env python3
"""Regression tests for logic-to-proof impact enforcement."""

import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_proof_impact
import proof_fingerprint
from check_claim_manifest import CLAIM_REPORT_SCHEMA, build_claim_report


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
        report = build_claim_report()
        claims = report["claims"]
        conditional_liveness = report["conditional_liveness"]
        current = {
            "algorithm": "sha256",
            "digest": "c" * 64,
            "input_count": 1,
        }
        summary = check_proof_impact.summarize_claim_report(
            claims, conditional_liveness
        )
        return {
            "schema": check_proof_impact.RECEIPT_SCHEMA,
            "required_stages": list(check_proof_impact.REQUIRED_STAGES),
            "stages": [
                {
                    "id": stage,
                    "status": "pass",
                    "source_fingerprint": current,
                }
                for stage in check_proof_impact.REQUIRED_STAGES
            ],
            "source_fingerprint": current,
            "claim_report_schema": CLAIM_REPORT_SCHEMA,
            "claims": claims,
            "conditional_liveness": conditional_liveness,
            "claim_summary": summary,
            "complete": True,
        }

    def check_receipt(self, receipt: dict[str, object]) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            with patch.object(
                check_proof_impact,
                "source_fingerprint",
                return_value=receipt["source_fingerprint"],
            ):
                check_proof_impact.check_receipt(path)

    def test_receipt_accepts_complete_recomputed_contents(self) -> None:
        self.check_receipt(self.valid_receipt())

    def test_release_summary_accepts_only_exact_bootstrap_claim_increment(self) -> None:
        expected = dict(check_proof_impact.EXPECTED_CLAIM_SUMMARY)
        bootstrap = dict(expected)
        bootstrap["total"] += 1
        bootstrap["release-ready"] += 1
        bootstrap["implementation-proved"] += 1
        self.assertTrue(check_proof_impact.release_summary_is_complete(expected))
        self.assertTrue(check_proof_impact.release_summary_is_complete(bootstrap))

        for field in ("total", "release-ready", "implementation-proved"):
            drift = dict(bootstrap)
            drift[field] += 1
            self.assertFalse(check_proof_impact.release_summary_is_complete(drift))

        downgraded = dict(bootstrap)
        downgraded["implementation-proved"] -= 1
        downgraded["production-linked"] += 1
        self.assertFalse(check_proof_impact.release_summary_is_complete(downgraded))

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
        for schema in (6, 4, 3.0, True):
            with self.subTest(schema=schema):
                wrong_schema = self.valid_receipt()
                wrong_schema["schema"] = schema
                with self.assertRaisesRegex(ValueError, "schema"):
                    self.check_receipt(wrong_schema)

        untyped = self.valid_receipt()
        untyped["stages"] = ["lean"]
        with self.assertRaisesRegex(ValueError, "typed records"):
            self.check_receipt(untyped)

    def test_receipt_rejects_missing_or_mixed_stage_fingerprints(self) -> None:
        missing = self.valid_receipt()
        missing["stages"][0].pop("source_fingerprint")
        with self.assertRaisesRegex(ValueError, "typed records"):
            self.check_receipt(missing)

        mixed = self.valid_receipt()
        mixed["stages"][0]["source_fingerprint"] = {
            "algorithm": "sha256",
            "digest": "d" * 64,
            "input_count": 1,
        }
        with self.assertRaisesRegex(ValueError, "do not match"):
            self.check_receipt(mixed)

    def test_receipt_rejects_stale_fingerprint(self) -> None:
        receipt = self.valid_receipt()
        stale = {
            "algorithm": "sha256",
            "digest": "d" * 64,
            "input_count": 1,
        }
        receipt["source_fingerprint"] = stale
        for stage in receipt["stages"]:
            stage["source_fingerprint"] = stale
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "receipt.json"
            path.write_text(json.dumps(receipt), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "stale"):
                check_proof_impact.check_receipt(path)

    def test_fingerprint_baseline_write_check_and_drift(self) -> None:
        current = {
            "algorithm": "sha256",
            "digest": "a" * 64,
            "input_count": 2,
        }
        changed = {
            "algorithm": "sha256",
            "digest": "b" * 64,
            "input_count": 2,
        }
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "baseline.json"
            with patch.object(proof_fingerprint, "source_fingerprint", return_value=current):
                self.assertEqual(proof_fingerprint.write_fingerprint(path), current)
                self.assertEqual(proof_fingerprint.check_fingerprint(path), current)
            with patch.object(proof_fingerprint, "source_fingerprint", return_value=changed):
                with self.assertRaisesRegex(ValueError, "proof run started"):
                    proof_fingerprint.check_fingerprint(path)

    def test_fingerprint_baseline_rejects_malformed_input(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "baseline.json"
            path.write_text('{"algorithm":"sha256","digest":"short","input_count":1}')
            with self.assertRaisesRegex(ValueError, "invalid shape"):
                proof_fingerprint.load_fingerprint(path)

    def test_fingerprint_excludes_certora_virtual_environment(self) -> None:
        verification = Path("/repo/verification")
        self.assertTrue(
            proof_fingerprint.excluded_verification_path(
                verification / "certora/.venv/lib/python/site-packages/cache.pyc",
                verification,
            )
        )
        self.assertFalse(
            proof_fingerprint.excluded_verification_path(
                verification / "certora/specs/Bridge.spec",
                verification,
            )
        )

    def test_receipt_rejects_forged_claims_and_summary(self) -> None:
        forged_claims = self.valid_receipt()
        forged_claims["claims"][0]["status"] = "forged"
        with self.assertRaisesRegex(ValueError, "summary does not match"):
            self.check_receipt(forged_claims)

        forged_summary = self.valid_receipt()
        forged_summary["claim_summary"]["release-ready"] += 1
        with self.assertRaisesRegex(ValueError, "summary does not match"):
            self.check_receipt(forged_summary)

    def test_receipt_rejects_policy_complete_claim_count_and_strength_drift(self) -> None:
        mutations = {
            "missing release-ready": lambda receipt: receipt["claims"][0].__setitem__(
                "status", "release-blocked"
            ),
            "model support": lambda receipt: receipt["claims"][0].__setitem__(
                "status", "model-support"
            ),
            "strength drift": lambda receipt: receipt["claims"][0].__setitem__(
                "evidence_strength", "abstract-proved"
            ),
        }
        for name, mutate in mutations.items():
            with self.subTest(name=name):
                receipt = self.valid_receipt()
                mutate(receipt)
                receipt["claim_summary"] = check_proof_impact.summarize_claim_report(
                    receipt["claims"], receipt["conditional_liveness"]
                )
                with self.assertRaisesRegex(ValueError, "completion flag"):
                    self.check_receipt(receipt)

    def test_conservative_fingerprint_covers_consumers_drivers_and_configs(self) -> None:
        inputs = {
            proof_fingerprint.logical_source_path(path, ROOT).as_posix()
            for path in check_proof_impact.fingerprint_inputs()
        }
        for relative in (
            "contracts/test/ProtocolVectors.t.sol",
            "integration/phase3.spec.ts",
            "ui/src/lib/mint-authorization.ts",
            "ui/src/lib/mint-authorization.test.ts",
            "scripts/check_tool_versions.sh",
            "scripts/plan007/evm-rpc-fault-injector",
            "verification/claims.tsv",
            "verification/halmos/uv.lock",
            "verification/lean/BridgeSpec/Claims.lean",
            "verification/generated/protocol-vectors.json",
            "verification/verus/fail/notification_ingestion_allowed.rs",
            "contracts/foundry.toml",
            ".node-version",
            "Cargo.lock",
            "icp.yaml",
            "lean-toolchain",
            "pnpm-lock.yaml",
            "pnpm-workspace.yaml",
            "ui/vitest.config.ts",
            "ui/pnpm-lock.yaml",
            "ui/pnpm-workspace.yaml",
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
            verification.mkdir(exist_ok=True)
            (verification / "claims.tsv").write_text("claims\n", encoding="utf-8")
            consumer = root / "contracts" / "test" / "ProtocolVectors.t.sol"
            consumer.parent.mkdir(parents=True)
            consumer.write_text("contract Consumer {}\n", encoding="utf-8")
            manifest = check_proof_impact.ImpactManifest((), ())

            before = check_proof_impact.source_fingerprint(root, manifest)
            consumer.write_text("contract ChangedConsumer {}\n", encoding="utf-8")
            after = check_proof_impact.source_fingerprint(root, manifest)

            self.assertNotEqual(before["digest"], after["digest"])

    def test_fingerprint_changes_when_proof_evidence_changes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative_root, _ in check_proof_impact.FINGERPRINT_SOURCE_ROOTS:
                (root / relative_root).mkdir(parents=True)
            for relative in check_proof_impact.FINGERPRINT_CONFIG_FILES:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"{relative}\n", encoding="utf-8")
            claims = root / "verification" / "claims.tsv"
            claims.write_text("before\n", encoding="utf-8")
            manifest = check_proof_impact.ImpactManifest((), ())

            before = check_proof_impact.source_fingerprint(root, manifest)
            claims.write_text("after\n", encoding="utf-8")
            after = check_proof_impact.source_fingerprint(root, manifest)

            self.assertNotEqual(before["digest"], after["digest"])

    def test_fingerprint_excludes_generated_receipts_and_build_state(self) -> None:
        inputs = {
            proof_fingerprint.logical_source_path(path, ROOT).as_posix()
            for path in check_proof_impact.fingerprint_inputs()
        }
        self.assertFalse(any(path.startswith("verification/output/") for path in inputs))
        self.assertFalse(any("/.lake/" in path for path in inputs))
        self.assertFalse(any("/.venv/" in path for path in inputs))
        self.assertFalse(any(path.startswith("verification/smt/out/") for path in inputs))
        self.assertFalse(any(path.startswith("verification/smt/cache/") for path in inputs))


if __name__ == "__main__":
    unittest.main()
