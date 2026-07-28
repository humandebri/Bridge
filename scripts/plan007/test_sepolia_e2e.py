#!/usr/bin/env python3
"""Regression tests for the Plan 007 staging evidence state machine."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("sepolia_e2e.py")
SPEC = importlib.util.spec_from_file_location("sepolia_e2e", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
sepolia_e2e = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sepolia_e2e)

H64 = "1" * 64
H64_B = "2" * 64
H64_C = "3" * 64
TX = f"0x{H64}"
TX_B = f"0x{H64_B}"
ADDRESS = f"0x{'4' * 40}"
ADDRESS_B = f"0x{'5' * 40}"
ADDRESS_C = f"0x{'6' * 40}"
SOURCE = "a" * 40


class SepoliaE2ETests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.manifest = self.root / "sepolia-e2e.json"
        self.local = self.root / "local-e2e.json"
        self.profile = self.root / "frontend-profile.json"
        self.local.write_text(
            json.dumps(
                {
                    "schema_version": 3,
                    "source_commit": SOURCE,
                    "bridge_wasm_sha256": H64,
                    "bridge_runtime_hash": TX,
                    "bsns_runtime_hash": TX_B,
                    "tests": {
                        "full_local_ci": "passed",
                        "real_frontend_e2e": "passed",
                        "canister_activation": "passed",
                        "timelock_24h": "passed",
                        "state_upgrade": "passed",
                    },
                }
            ),
            encoding="utf-8",
        )
        self.profile.write_text(
            json.dumps(
                {
                    "environment": "sepolia-staging",
                    "testOnly": True,
                    "chainId": 84532,
                    "evmRpcCanisterId": "7hfb6-caaaa-aaaar-qadga-cai",
                    "bridgeCanisterId": "aaaaa-aa",
                    "ledgerCanisterId": "ryjl3-tyaaa-aaaaa-aaaba-cai",
                    "indexCanisterId": "qhbym-qaaaa-aaaaa-aaafq-cai",
                }
            ),
            encoding="utf-8",
        )
        sepolia_e2e.initialize(self.manifest, self.local, self.profile)

    def tearDown(self) -> None:
        self.temp.cleanup()

    def artifact(self, stage: str) -> list[dict[str, str]]:
        path = self.root / "artifacts" / f"{stage}.json"
        path.parent.mkdir(exist_ok=True)
        path.write_text("{}\n", encoding="utf-8")
        return [{"path": f"artifacts/{stage}.json", "sha256": sepolia_e2e.digest(path), "kind": "raw"}]

    def details(self, stage: str) -> dict[str, object]:
        if stage == "preflight":
            return {
                "chain_id": 84532,
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "bridge_canister_id": "aaaaa-aa",
                "ledger_canister_id": "ryjl3-tyaaa-aaaaa-aaaba-cai",
                "index_canister_id": "qhbym-qaaaa-aaaaa-aaafq-cai",
                "ledger_symbol": "TICRC1",
                "ledger_decimals": 8,
                "ledger_fee": 10_000,
                "index_ledger_id": "ryjl3-tyaaa-aaaaa-aaaba-cai",
                "controller_principals": ["aaaaa-aa"],
                "cycles_balance": 1,
                "base_deposits_paused": True,
                "base_withdrawals_paused": True,
                "canister_deposits_paused": True,
                "configured_rpc_url_sha256": [H64, H64_B, H64_C],
            }
        if stage == "install":
            return {"install_mode": "reinstall", "module_sha256": H64, "cycles_balance": 1, "controller_principals": ["aaaaa-aa"]}
        if stage == "initialize":
            return {
                "schema_version": 25,
                "chain_id": 84532,
                "ledger_canister_id": "ryjl3-tyaaa-aaaaa-aaaba-cai",
                "index_canister_id": "qhbym-qaaaa-aaaaa-aaafq-cai",
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "expected_bridge_signer": ADDRESS,
                "governance_operator": ADDRESS_B,
                "canister_deposits_paused": True,
                "storage_integrity": "ok",
            }
        if stage == "contracts":
            return {
                "bridge_address": ADDRESS,
                "bsns_address": ADDRESS_B,
                "timelock_address": ADDRESS_C,
                "bridge_runtime_hash": TX,
                "bsns_runtime_hash": TX_B,
                "deployment_block": 1,
                "deployment_transaction_hashes": [TX],
                "mint_signer": ADDRESS,
                "governance_operator": ADDRESS_B,
                "deployer_roles_zero": True,
            }
        if stage == "activation_schedule":
            return {
                "operation_id": TX,
                "schedule_transaction_hash": TX_B,
                "finalized_block_number": 1,
                "finalized_block_hash": TX,
                "early_execute_reverted": True,
            }
        if stage == "activation_execute":
            return {
                "delay_seconds": 86_400,
                "execute_transaction_hash": TX,
                "finalized_block_number": 2,
                "finalized_block_hash": TX_B,
                "base_deposits_paused": False,
                "base_withdrawals_paused": False,
                "canister_deposits_paused": False,
                "pending_timelock_operations": 0,
            }
        if stage == "frontend_publish":
            profile_hash = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]["frontend_profile_sha256"]
            return {
                "url": "https://kinic-bridge-ui-test.example.test",
                "deployment_id": "deployment-1",
                "profile_sha256": profile_hash,
                "test_banner_visible": True,
                "runtime_verification_fresh": True,
            }
        if stage == "wallet_e2e":
            flow = {
                "operation_id": TX,
                "ledger_block": 1,
                "base_transaction_hash": TX_B,
                "finalized_block_number": 2,
                "finalized_block_hash": TX,
                "completed": True,
            }
            return {
                "chrome_version": "1",
                "wallet_versions": {"Plug": "1", "OISY": "1", "MetaMask": "1", "Rabby": "1", "WalletConnect": "1"},
                "deposits": [{**flow, "wallet": "Plug"}, {**flow, "wallet": "OISY"}],
                "withdrawals": [{**flow, "wallet": "MetaMask"}, {**flow, "wallet": "Rabby"}],
                "walletconnect": {"connected": True, "rejection_safe": True, "account_change_safe": True, "chain_change_safe": True, "csp_clean": True},
                "failure_checks": {
                    "wallet_rejection": True,
                    "popup_close": True,
                    "reload": True,
                    "duplicate_payload": True,
                    "conflicting_payload": True,
                    "sequence_gap": True,
                    "two_tab_lease": True,
                    "wallet_disconnect": True,
                    "account_change": True,
                    "chain_change": True,
                    "runtime_mismatch": True,
                    "notification_recovery": True,
                },
                "same_wasm_upgrade": {"before_state_sha256": H64, "after_state_sha256": H64, "storage_integrity": "ok", "verified": True},
            }
        if stage == "rpc_rehearsal":
            return {
                "manifest_sha256": H64,
                "state": "COMPLETE",
                "complete": True,
                "scenarios": sorted(sepolia_e2e.RPC_SCENARIOS),
                "providers_restored": True,
            }
        if stage == "final_pause":
            return {
                "base_deposits_paused": True,
                "base_withdrawals_paused": True,
                "canister_deposits_paused": True,
                "pending_timelock_operations": 0,
                "pending_deposits": 0,
                "pending_withdrawals": 0,
                "providers_restored": True,
                "finalized_block_number": 3,
                "finalized_block_hash": TX,
            }
        raise AssertionError(stage)

    def evidence(self, stage: str) -> Path:
        path = self.root / f"{stage}-evidence.json"
        path.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "stage": stage,
                    "observed_at": "2026-07-24T00:00:00Z",
                    "source_commit": SOURCE,
                    "artifacts": self.artifact(stage),
                    "details": self.details(stage),
                }
            ),
            encoding="utf-8",
        )
        return path

    def test_complete_manifest_verifies_all_files_and_stages(self) -> None:
        for stage in sepolia_e2e.STAGES:
            sepolia_e2e.record(self.manifest, self.evidence(stage))
        manifest = sepolia_e2e.load_object(self.manifest)
        sepolia_e2e.validate_manifest(manifest, self.manifest, require_complete=True, verify_files=True)
        self.assertEqual(manifest["state"], "COMPLETE")
        self.assertTrue(manifest["complete"])

    def test_out_of_order_stage_is_rejected(self) -> None:
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "expected stage preflight"):
            sepolia_e2e.record(self.manifest, self.evidence("install"))

    def test_upgrade_hash_drift_is_rejected(self) -> None:
        details = self.details("wallet_e2e")
        details["same_wasm_upgrade"]["after_state_sha256"] = H64_B
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "did not preserve canonical state"):
            sepolia_e2e.validate_wallet_e2e(details)

    def test_artifact_hash_drift_is_rejected(self) -> None:
        evidence = self.evidence("preflight")
        artifact = self.root / "artifacts/preflight.json"
        artifact.write_text('{"changed":true}\n', encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "does not match"):
            sepolia_e2e.record(self.manifest, evidence)

    def test_initialization_allows_only_fresh_local_evidence_diff(self) -> None:
        self.manifest.unlink()
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        subprocess.run(["git", "config", "user.name", "Plan 007 Test"], cwd=self.root, check=True)
        subprocess.run(["git", "config", "user.email", "plan007@example.invalid"], cwd=self.root, check=True)
        subprocess.run(["git", "add", self.local.name, self.profile.name], cwd=self.root, check=True)
        subprocess.run(["git", "commit", "-qm", "test fixture"], cwd=self.root, check=True)
        head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.root, check=True, text=True, stdout=subprocess.PIPE).stdout.strip()
        local = json.loads(self.local.read_text(encoding="utf-8"))
        local["source_commit"] = head
        self.local.write_text(json.dumps(local), encoding="utf-8")
        sepolia_e2e.initialize(self.manifest, self.local, self.profile, self.root)
        self.manifest.unlink()
        (self.root / "unexpected.txt").write_text("dirty\n", encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "allows only"):
            sepolia_e2e.initialize(self.manifest, self.local, self.profile, self.root)


if __name__ == "__main__":
    unittest.main()
