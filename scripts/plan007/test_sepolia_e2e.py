#!/usr/bin/env python3
"""Regression tests for the Plan 007 staging evidence state machine."""

from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("sepolia_e2e.py")
DRIVER_PATH = Path(__file__).with_name("staging-e2e-driver.sh")
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
OBSOLETE_POLICY = sepolia_e2e.OBSOLETE_REPLACEMENT_POLICY


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
                    "schema_version": 7,
                    "environment_mode": "short-delay-test-only",
                    "activation_timelock_delay_seconds": 300,
                    "deployment_instance_id": TX,
                    "source_commit": SOURCE,
                    "bridge_wasm_sha256": H64,
                    "bridge_runtime_template_sha256": TX,
                    "bsns_runtime_template_sha256": TX_B,
                    "state_upgrade": {
                        "verified": True,
                        "before": {"status": {"schema_version": 31}},
                        "after": {"status": {"schema_version": 31}},
                    },
                    "tests": {
                        "full_local_ci": "passed",
                        "real_frontend_e2e": "passed",
                        "canister_activation": "passed",
                        "timelock_delay_enforced": "passed",
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
                    "environmentMode": "short-delay-test-only",
                    "activationTimelockDelaySeconds": 300,
                    "chainId": 84532,
                    "evmRpcCanisterId": "7hfb6-caaaa-aaaar-qadga-cai",
                    "bridgeCanisterId": OBSOLETE_POLICY["bridge_canister_id"],
                    "deploymentInstanceId": TX_B,
                    "ledgerCanisterId": "ryjl3-tyaaa-aaaaa-aaaba-cai",
                    "indexCanisterId": "qhbym-qaaaa-aaaaa-aaafq-cai",
                    "bridgeAddress": ADDRESS,
                    "bsnsAddress": ADDRESS_B,
                    "timelockAddress": ADDRESS_C,
                    "bridgeRuntimeHash": TX,
                    "bsnsRuntimeHash": TX_B,
                    "expected_bridge_signer": ADDRESS,
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
        artifacts = [
            {
                "path": f"artifacts/{stage}.json",
                "sha256": sepolia_e2e.digest(path),
                "kind": "raw",
            }
        ]
        if stage == "preflight":
            artifacts.extend(self.reinstall_artifacts())
        return artifacts

    def reinstall_artifacts(
        self,
        live: dict[str, object] | None = None,
        check: dict[str, object] | None = None,
        status: dict[str, object] | None = None,
    ) -> list[dict[str, str]]:
        if live is None:
            live = {"schema_version": 31, "deployment_instance_id": [17] * 32}
        if check is None:
            check = {
                "replacement_mode": "current-schema-reinstall",
                "live_schema_version": 31,
                "previous_deployment_instance_id": TX,
                "live_module_hash": TX,
                "next": TX_B,
            }
        if status is None:
            status = {
                "module_hash": TX,
                "controller_principals": ["aaaaa-aa"],
                "cycles_balance": 1,
            }
        artifacts = self.root / "artifacts"
        artifacts.mkdir(exist_ok=True)
        live_path = artifacts / "live-public-config.json"
        check_path = artifacts / "reinstall-instance-check.json"
        status_path = artifacts / "live-canister-status.json"
        live_path.write_text(json.dumps(live) + "\n", encoding="utf-8")
        check_path.write_text(json.dumps(check) + "\n", encoding="utf-8")
        status_path.write_text(json.dumps(status) + "\n", encoding="utf-8")
        return [
            {
                "path": "artifacts/live-public-config.json",
                "sha256": sepolia_e2e.digest(live_path),
                "kind": sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND,
            },
            {
                "path": "artifacts/reinstall-instance-check.json",
                "sha256": sepolia_e2e.digest(check_path),
                "kind": sepolia_e2e.REINSTALL_INSTANCE_CHECK_ARTIFACT_KIND,
            },
            {
                "path": "artifacts/live-canister-status.json",
                "sha256": sepolia_e2e.digest(status_path),
                "kind": sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND,
            },
        ]

    def obsolete_reinstall_artifacts(
        self,
        *,
        bridge_status: dict[str, object] | None = None,
        activation_status: dict[str, object] | None = None,
        integrity: dict[str, object] | None = None,
        pause_mutation: object | None = None,
        disposition_mutation: object | None = None,
    ) -> list[dict[str, str]]:
        live = {
            "schema_version": 30,
            "deployment_instance_id": OBSOLETE_POLICY[
                "previous_deployment_instance_id"
            ],
        }
        check = {
            "replacement_mode": "obsolete-schema-reinstall",
            "live_schema_version": 30,
            "previous_deployment_instance_id": OBSOLETE_POLICY[
                "previous_deployment_instance_id"
            ],
            "live_module_hash": OBSOLETE_POLICY["module_hash"],
            "next": TX_B,
        }
        canister_status = {
            "module_hash": OBSOLETE_POLICY["module_hash"],
            "controller_principals": ["aaaaa-aa"],
            "cycles_balance": 1,
        }
        artifacts = self.reinstall_artifacts(live, check, canister_status)
        snapshots: dict[str, tuple[str, dict[str, object]]] = {
            "bridge_status_sha256": (
                sepolia_e2e.LIVE_BRIDGE_STATUS_ARTIFACT_KIND,
                bridge_status
                or {
                    "deposits": 2,
                    "withdrawals": 0,
                    "pending_ledger_operations": 0,
                    "reconciliation_holds": 0,
                    "reserved_deposit_mint_operations": 2,
                    "reserved_deposit_mint_amount": 1_000_000_000,
                    "unpaid_withdrawal_count": 0,
                    "unpaid_withdrawal_amount_out": 0,
                },
            ),
            "activation_status_sha256": (
                sepolia_e2e.LIVE_ACTIVATION_STATUS_ARTIFACT_KIND,
                activation_status or {"pending_timelock_operations": 0},
            ),
            "storage_integrity_sha256": (
                sepolia_e2e.LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND,
                integrity or {"result": "ok"},
            ),
            "ledger_balance_sha256": (
                sepolia_e2e.LIVE_LEDGER_BALANCE_ARTIFACT_KIND,
                {"balance_raw": 600_000_000},
            ),
        }
        snapshot_hashes: dict[str, str] = {
            "public_config_sha256": next(
                item["sha256"]
                for item in artifacts
                if item["kind"] == sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND
            ),
            "canister_status_sha256": next(
                item["sha256"]
                for item in artifacts
                if item["kind"] == sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND
            ),
        }
        artifact_root = self.root / "artifacts"
        for hash_field, (kind, value) in snapshots.items():
            path = artifact_root / f"{kind}.json"
            path.write_text(json.dumps(value) + "\n", encoding="utf-8")
            sha256 = sepolia_e2e.digest(path)
            snapshot_hashes[hash_field] = sha256
            artifacts.append(
                {"path": f"artifacts/{kind}.json", "sha256": sha256, "kind": kind}
            )
        pause_evidence = {
            "schema_version": 2,
            "environment": "sepolia-staging",
            "capture_id": "11111111-1111-4111-8111-111111111111",
            "capture_started_at": "2026-07-24T00:00:00Z",
            "observed_at": "2026-07-24T00:00:30Z",
            "bridge_canister_id": OBSOLETE_POLICY["bridge_canister_id"],
            "chain_id": 84532,
            "bridge_address": ADDRESS,
            "providers": [
                {
                    "provider_url_sha256": digest_char * 64,
                    "chain_id": 84532,
                    "finalized_block_number": 101,
                    "finalized_block_hash": TX_B,
                    "observed_at": "2026-07-24T00:00:10Z",
                    "actions": [
                        {
                            "kind": kind,
                            "transaction_hash": TX if kind == "PauseDepositMints" else TX_B,
                            "block_number": 100,
                            "block_hash": TX,
                            "receipt_status": 1,
                            "target": ADDRESS,
                            "calldata_hex": expected["calldata_hex"],
                            "event_topic": expected["event_topic"],
                            "event_observed": True,
                            "canonical_probe_succeeded": True,
                        }
                        for kind, expected in sepolia_e2e.OBSOLETE_PAUSE_ACTIONS.items()
                    ],
                }
                for digest_char in ("1", "2", "3")
            ],
            "ic_pause": {
                "method": "pause_new_deposits",
                "response": "Ok",
                "audit_sequence": 7,
                "audit_kind": "DepositsPaused",
                "audit_caller": "aaaaa-aa",
                "status_deposits_paused": True,
                "status_last_audit_sequence": 7,
                "observed_at": "2026-07-24T00:00:20Z",
            },
            "ic_live": {
                "observed_at": "2026-07-24T00:00:25Z",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
                "live_module_hash": OBSOLETE_POLICY["module_hash"],
                "status_deposits_paused": True,
            },
            "complete": True,
        }
        if callable(pause_mutation):
            pause_mutation(pause_evidence)
        pause_path = artifact_root / "obsolete-pause-evidence.json"
        pause_path.write_text(json.dumps(pause_evidence) + "\n", encoding="utf-8")
        pause_sha256 = sepolia_e2e.digest(pause_path)
        snapshot_hashes["pause_evidence_sha256"] = pause_sha256
        artifacts.append(
            {
                "path": "artifacts/obsolete-pause-evidence.json",
                "sha256": pause_sha256,
                "kind": sepolia_e2e.OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND,
            }
        )
        disposition: dict[str, object] = {
            "schema_version": 1,
            "environment": "sepolia-staging",
            "observed_at": "2026-07-24T00:00:00Z",
            "bridge_canister_id": OBSOLETE_POLICY["bridge_canister_id"],
            "live_schema_version": 30,
            "previous_deployment_instance_id": OBSOLETE_POLICY[
                "previous_deployment_instance_id"
            ],
            "module_hash": OBSOLETE_POLICY["module_hash"],
            **snapshot_hashes,
            "counts": dict(snapshots["bridge_status_sha256"][1]),
            "pending_timelock_operations": 0,
            "ledger_balance_raw": 600_000_000,
            "disposition": "discard-test-state",
            "complete": True,
        }
        if callable(disposition_mutation):
            disposition_mutation(disposition)
        disposition_path = artifact_root / "obsolete-state-disposition.json"
        disposition_path.write_text(json.dumps(disposition) + "\n", encoding="utf-8")
        artifacts.append(
            {
                "path": "artifacts/obsolete-state-disposition.json",
                "sha256": sepolia_e2e.digest(disposition_path),
                "kind": sepolia_e2e.OBSOLETE_STATE_DISPOSITION_ARTIFACT_KIND,
            }
        )
        return artifacts

    def details(self, stage: str) -> dict[str, object]:
        if stage == "preflight":
            return {
                "chain_id": 84532,
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "bridge_canister_id": OBSOLETE_POLICY["bridge_canister_id"],
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
                "replacement_mode": "current-schema-reinstall",
                "live_schema_version": 31,
                "previous_deployment_instance_id": TX,
            }
        if stage == "install":
            return {"install_mode": "reinstall", "module_sha256": H64, "cycles_balance": 1, "controller_principals": ["aaaaa-aa"]}
        if stage == "initialize":
            return {
                "schema_version": 31,
                "deployment_instance_id": TX_B,
                "chain_id": 84532,
                "ledger_canister_id": "ryjl3-tyaaa-aaaaa-aaaba-cai",
                "index_canister_id": "qhbym-qaaaa-aaaaa-aaafq-cai",
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "expected_bridge_signer": ADDRESS,
                "bridge_address": ADDRESS,
                "timelock_address": ADDRESS_C,
                "expected_bridge_runtime_sha256": TX,
                "governance_operator": ADDRESS_B,
                "canister_deposits_paused": True,
                "storage_integrity": "ok",
            }
        if stage == "contracts":
            return {
                "bridge_address": ADDRESS,
                "bsns_address": ADDRESS_B,
                "timelock_address": ADDRESS_C,
                "bridge_runtime_template_sha256": TX,
                "bsns_runtime_template_sha256": TX_B,
                "bridge_runtime_sha256": TX,
                "bsns_runtime_sha256": TX_B,
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
                "delay_seconds": 300,
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
        if stage == "smoke_e2e":
            return {
                "ic_wallet": "OISY",
                "evm_wallet": "MetaMask",
                "deposit_id": TX,
                "deposit_transaction_hash": TX_B,
                "withdrawal_id": 1,
                "withdrawal_transaction_hash": TX,
                "reload_state_matched": True,
                "base_deposits_paused": False,
                "base_withdrawals_paused": False,
                "canister_deposits_paused": False,
                "pending_timelock_operations": 0,
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
                "state": "EXTENDED_COMPLETE",
                "launch_ready": True,
                "extended_complete": True,
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
                    "schema_version": 6,
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
        self.assertEqual(manifest["state"], "SHORT_DELAY_COMPLETE")
        self.assertTrue(manifest["complete"])

    def test_out_of_order_stage_is_rejected(self) -> None:
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "expected stage preflight"):
            sepolia_e2e.record(self.manifest, self.evidence("install"))

    def test_binding_uses_profile_instance_instead_of_local_evidence(self) -> None:
        manifest = json.loads(self.manifest.read_text(encoding="utf-8"))
        self.assertEqual(manifest["binding"]["deployment_instance_id"], TX_B)

    def test_current_schema_preflight_accepts_distinct_previous_instance(self) -> None:
        sepolia_e2e.validate_preflight(
            self.details("preflight"),
            json.loads(self.manifest.read_text(encoding="utf-8"))["binding"],
            self.artifact("preflight"),
            self.manifest,
        )

    def test_obsolete_schema_preflight_accepts_audited_test_state_discard(self) -> None:
        details = self.details("preflight")
        details.update(
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            }
        )
        sepolia_e2e.validate_preflight(
            details,
            json.loads(self.manifest.read_text(encoding="utf-8"))["binding"],
            self.obsolete_reinstall_artifacts(),
            self.manifest,
        )

    def test_obsolete_schema_preflight_requires_every_snapshot(self) -> None:
        details = self.details("preflight")
        details.update(
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            }
        )
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        artifacts = self.obsolete_reinstall_artifacts()
        required = (
            sepolia_e2e.OBSOLETE_STATE_DISPOSITION_ARTIFACT_KIND,
            sepolia_e2e.OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND,
            sepolia_e2e.LIVE_BRIDGE_STATUS_ARTIFACT_KIND,
            sepolia_e2e.LIVE_ACTIVATION_STATUS_ARTIFACT_KIND,
            sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND,
            sepolia_e2e.LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND,
            sepolia_e2e.LIVE_LEDGER_BALANCE_ARTIFACT_KIND,
        )
        for kind in required:
            with self.subTest(kind=kind):
                missing = [item for item in artifacts if item["kind"] != kind]
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "exactly one"):
                    sepolia_e2e.validate_preflight(details, binding, missing, self.manifest)

    def test_obsolete_schema_preflight_refuses_unsettled_liabilities(self) -> None:
        details = self.details("preflight")
        details.update(
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            }
        )
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        for field in (
            "withdrawals",
            "pending_ledger_operations",
            "reconciliation_holds",
            "unpaid_withdrawal_count",
            "unpaid_withdrawal_amount_out",
        ):
            with self.subTest(field=field):
                status = {
                    "deposits": 2,
                    "withdrawals": 0,
                    "pending_ledger_operations": 0,
                    "reconciliation_holds": 0,
                    "reserved_deposit_mint_operations": 2,
                    "reserved_deposit_mint_amount": 1_000_000_000,
                    "unpaid_withdrawal_count": 0,
                    "unpaid_withdrawal_amount_out": 0,
                }
                status[field] = 1
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "refuses to discard"):
                    sepolia_e2e.validate_preflight(
                        details,
                        binding,
                        self.obsolete_reinstall_artifacts(bridge_status=status),
                        self.manifest,
                    )

    def test_obsolete_schema_preflight_rejects_timelock_integrity_and_disposition_drift(self) -> None:
        details = self.details("preflight")
        details.update(
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            }
        )
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        fixture_args = (
            {"activation_status": {"pending_timelock_operations": 1}},
            {"integrity": {"result": "failed"}},
            {
                "disposition_mutation": lambda value: value.update(
                    {"disposition": "recover-test-state"}
                )
            },
        )
        for kwargs in fixture_args:
            with self.assertRaises(sepolia_e2e.EvidenceError):
                sepolia_e2e.validate_preflight(
                    details,
                    binding,
                    self.obsolete_reinstall_artifacts(**kwargs),
                    self.manifest,
                )

    def test_obsolete_schema_preflight_rejects_snapshot_hash_drift(self) -> None:
        details = self.details("preflight")
        details.update(
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            }
        )
        artifacts = self.obsolete_reinstall_artifacts()
        status_path = self.root / "artifacts/live-bridge-status.json"
        status_path.write_text('{"changed":true}\n', encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "sha256 does not match"):
            sepolia_e2e.validate_preflight(
                details,
                json.loads(self.manifest.read_text(encoding="utf-8"))["binding"],
                artifacts,
                self.manifest,
            )

    def test_obsolete_schema_reinstall_is_limited_to_the_reviewed_tuple(self) -> None:
        live = {
            "schema_version": OBSOLETE_POLICY["live_schema_version"],
            "deployment_instance_id": OBSOLETE_POLICY[
                "previous_deployment_instance_id"
            ],
        }
        status = {"module_hash": OBSOLETE_POLICY["module_hash"]}
        cases = (
            (live, status, "aaaaa-aa"),
            ({**live, "deployment_instance_id": TX}, status, OBSOLETE_POLICY["bridge_canister_id"]),
            (live, {"module_hash": TX}, OBSOLETE_POLICY["bridge_canister_id"]),
        )
        for candidate_live, candidate_status, canister_id in cases:
            with self.subTest(live=candidate_live, status=candidate_status, canister_id=canister_id):
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "reviewed replacement policy"):
                    sepolia_e2e.reinstall_instance_check(
                        TX_B,
                        candidate_live,
                        candidate_status,
                        canister_id,
                    )

    def test_obsolete_pause_evidence_rejects_unverified_observations(self) -> None:
        details = self.details("preflight")
        details.update(
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            }
        )
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]

        def first_action(value: dict[str, object]) -> dict[str, object]:
            return value["providers"][0]["actions"][0]

        def spread_observations(value: dict[str, object]) -> None:
            value["observed_at"] = "2026-07-24T00:01:10Z"
            value["providers"][0]["observed_at"] = "2026-07-24T00:00:00Z"
            value["ic_live"]["observed_at"] = "2026-07-24T00:01:01Z"

        mutations = {
            "invalid capture ID": lambda value: value.update({"capture_id": "not-a-capture-id"}),
            "stale capture": lambda value: value.update({"observed_at": "2026-07-24T00:06:00Z"}),
            "future capture": lambda value: value.update({"observed_at": "2999-07-24T00:00:30Z"}),
            "observation interval": spread_observations,
            "instance drift": lambda value: value["ic_live"].update({"previous_deployment_instance_id": TX}),
            "module drift": lambda value: value["ic_live"].update({"live_module_hash": TX}),
            "post-pause status drift": lambda value: value["ic_live"].update({"status_deposits_paused": False}),
            "reverted receipt": lambda value: first_action(value).update({"receipt_status": 0}),
            "wrong target": lambda value: first_action(value).update({"target": ADDRESS_B}),
            "wrong calldata": lambda value: first_action(value).update({"calldata_hex": "0x00000000"}),
            "missing event": lambda value: first_action(value).update({"event_observed": False}),
            "IC audit mismatch": lambda value: value["ic_pause"].update({"status_last_audit_sequence": 8}),
            "IC status mismatch": lambda value: value["ic_pause"].update({"status_deposits_paused": False}),
            "quorum shortage": lambda value: (
                value["providers"][1].update({"finalized_block_hash": TX}),
                value["providers"][2].update({"finalized_block_hash": f"0x{'4' * 64}"}),
            ),
        }
        for name, mutation in mutations.items():
            with self.subTest(name=name):
                with self.assertRaises(sepolia_e2e.EvidenceError):
                    sepolia_e2e.validate_preflight(
                        details,
                        binding,
                        self.obsolete_reinstall_artifacts(pause_mutation=mutation),
                        self.manifest,
                    )

        expected_check = sepolia_e2e.reinstall_instance_check(
            binding["deployment_instance_id"],
            {
                "schema_version": 30,
                "deployment_instance_id": OBSOLETE_POLICY[
                    "previous_deployment_instance_id"
                ],
            },
            {"module_hash": OBSOLETE_POLICY["module_hash"]},
            binding["bridge_canister_id"],
        )
        with self.assertRaises(sepolia_e2e.EvidenceError):
            sepolia_e2e.validate_obsolete_pause_evidence(
                {
                    "base_deposits_paused": True,
                    "base_withdrawals_paused": True,
                    "canister_deposits_paused": True,
                },
                binding,
                expected_check,
            )

    def test_live_capture_is_the_only_cli_path_that_writes_pause_evidence(self) -> None:
        artifacts = self.obsolete_reinstall_artifacts()
        paths = {
            artifact["kind"]: self.manifest.parent / artifact["path"]
            for artifact in artifacts
        }
        pause_value = json.loads(
            paths[sepolia_e2e.OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND].read_text(
                encoding="utf-8"
            )
        )
        output = self.root / "captured/obsolete-pause-evidence.json"
        capture_config = self.root / "capture-config.json"
        capture_config.write_text("{}\n", encoding="utf-8")
        commands: list[list[str]] = []

        def collector(command: list[str], **options: object) -> subprocess.CompletedProcess[str]:
            commands.append(command)
            self.assertEqual(options, {"check": True, "capture_output": True, "text": True})
            return subprocess.CompletedProcess(command, 0, json.dumps(pause_value), "")

        sepolia_e2e.capture_obsolete_pause_evidence(
            capture_config,
            output,
            self.profile,
            paths[sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND],
            paths[sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND],
            collector,
        )
        self.assertEqual(json.loads(output.read_text(encoding="utf-8")), pause_value)
        self.assertEqual(commands[0][0:2], ["node", str(sepolia_e2e.OBSOLETE_PAUSE_COLLECTOR_PATH)])
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "refusing to overwrite"):
            sepolia_e2e.capture_obsolete_pause_evidence(
                capture_config,
                output,
                self.profile,
                paths[sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND],
                paths[sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND],
                collector,
            )

        help_result = subprocess.run(
            [sys.executable, str(MODULE_PATH), "--help"],
            check=True,
            capture_output=True,
            text=True,
        )
        self.assertIn("capture-obsolete-pause", help_result.stdout)
        self.assertNotIn("verify-obsolete-pause", help_result.stdout)
        driver_result = subprocess.run(
            [str(DRIVER_PATH), "verify-obsolete-pause"],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(driver_result.returncode, 2)
        self.assertNotIn("verify-obsolete-pause", driver_result.stderr)

    def test_failed_live_capture_does_not_leave_pause_evidence(self) -> None:
        artifacts = self.obsolete_reinstall_artifacts()
        paths = {
            artifact["kind"]: self.manifest.parent / artifact["path"]
            for artifact in artifacts
        }
        capture_config = self.root / "capture-config.json"
        capture_config.write_text("{}\n", encoding="utf-8")
        output = self.root / "captured/obsolete-pause-evidence.json"
        for stdout in ("not-json", "[]"):
            with self.subTest(stdout=stdout):
                with self.assertRaises(sepolia_e2e.EvidenceError):
                    sepolia_e2e.capture_obsolete_pause_evidence(
                        capture_config,
                        output,
                        self.profile,
                        paths[sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND],
                        paths[sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND],
                        lambda command, **_options: subprocess.CompletedProcess(
                            command, 0, stdout, ""
                        ),
                    )
                self.assertFalse(output.exists())

        def failed_collector(command: list[str], **_options: object) -> None:
            raise subprocess.CalledProcessError(1, command)

        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "collector failed"):
            sepolia_e2e.capture_obsolete_pause_evidence(
                capture_config,
                output,
                self.profile,
                paths[sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND],
                paths[sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND],
                failed_collector,
            )
        self.assertFalse(output.exists())

        pause_value = json.loads(
            paths[sepolia_e2e.OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND].read_text(
                encoding="utf-8"
            )
        )
        wrong_live = self.root / "wrong-live.json"
        wrong_live.write_text(
            json.dumps({"schema_version": 31, "deployment_instance_id": TX}),
            encoding="utf-8",
        )
        with self.assertRaises(sepolia_e2e.EvidenceError):
            sepolia_e2e.capture_obsolete_pause_evidence(
                capture_config,
                output,
                self.profile,
                wrong_live,
                paths[sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND],
                lambda command, **_options: subprocess.CompletedProcess(
                    command, 0, json.dumps(pause_value), ""
                ),
            )
        self.assertFalse(output.exists())

    def test_current_schema_preflight_rejects_obsolete_disposition(self) -> None:
        artifacts = self.reinstall_artifacts()
        disposition_path = self.root / "artifacts/obsolete-state-disposition.json"
        disposition_path.write_text("{}\n", encoding="utf-8")
        artifacts.append(
            {
                "path": "artifacts/obsolete-state-disposition.json",
                "sha256": sepolia_e2e.digest(disposition_path),
                "kind": sepolia_e2e.OBSOLETE_STATE_DISPOSITION_ARTIFACT_KIND,
            }
        )
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "must not include"):
            sepolia_e2e.validate_preflight(
                self.details("preflight"),
                json.loads(self.manifest.read_text(encoding="utf-8"))["binding"],
                artifacts,
                self.manifest,
            )

    def test_preflight_requires_all_three_pause_postconditions(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        for field in (
            "base_deposits_paused",
            "base_withdrawals_paused",
            "canister_deposits_paused",
        ):
            with self.subTest(field=field):
                details = self.details("preflight")
                details[field] = False
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "must be true"):
                    sepolia_e2e.validate_preflight(
                        details,
                        binding,
                        self.reinstall_artifacts(),
                        self.manifest,
                    )

    def test_checked_in_json_schema_tracks_manifest_schema_v6(self) -> None:
        schema_path = MODULE_PATH.parents[2] / "deployments/sepolia-staging/sepolia-e2e.schema.json"
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["schema_version"]["const"], 6)
        self.assertIn("obsolete-state-disposition", schema_path.read_text(encoding="utf-8"))
        self.assertIn("obsolete-pause-evidence", schema_path.read_text(encoding="utf-8"))

    def test_v31_preflight_requires_a_distinct_previous_instance(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        details = self.details("preflight")
        details["live_schema_version"] = 31
        details["previous_deployment_instance_id"] = TX
        artifacts = self.reinstall_artifacts(
            {"schema_version": 31, "deployment_instance_id": [17] * 32},
            {
                "replacement_mode": "current-schema-reinstall",
                "live_schema_version": 31,
                "previous_deployment_instance_id": TX,
                "live_module_hash": TX,
                "next": TX_B,
            },
        )
        sepolia_e2e.validate_preflight(details, binding, artifacts, self.manifest)
        for live in (
            {"schema_version": 31},
            {"schema_version": 31, "deployment_instance_id": TX_B},
            {"schema_version": 31, "deployment_instance_id": f"0x{'0' * 64}"},
            {"schema_version": 31, "deployment_instance_id": "0x11"},
            {"schema_version": 31, "deployment_instance_id": [17] * 31},
            {"schema_version": 28, "deployment_instance_id": TX},
        ):
            invalid_artifacts = self.reinstall_artifacts(live)
            with self.assertRaises(sepolia_e2e.EvidenceError):
                sepolia_e2e.validate_preflight(
                    details,
                    binding,
                    invalid_artifacts,
                    self.manifest,
                )

    def test_preflight_requires_unique_reinstall_artifacts(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        artifacts = self.artifact("preflight")
        required_kinds = (
            sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND,
            sepolia_e2e.REINSTALL_INSTANCE_CHECK_ARTIFACT_KIND,
        )
        for kind in required_kinds:
            missing = [artifact for artifact in artifacts if artifact["kind"] != kind]
            with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "exactly one"):
                sepolia_e2e.validate_preflight(
                    self.details("preflight"),
                    binding,
                    missing,
                    self.manifest,
                )
            duplicate = artifacts + [
                next(artifact.copy() for artifact in artifacts if artifact["kind"] == kind)
            ]
            with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "exactly one"):
                sepolia_e2e.validate_preflight(
                    self.details("preflight"),
                    binding,
                    duplicate,
                    self.manifest,
                )

    def test_preflight_rejects_artifact_and_summary_drift(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        details = self.details("preflight")
        artifacts = self.reinstall_artifacts(
            {"schema_version": 31, "deployment_instance_id": TX},
            {
                "replacement_mode": "current-schema-reinstall",
                "live_schema_version": 31,
                "previous_deployment_instance_id": TX,
                "live_module_hash": TX,
                "next": TX,
            },
        )
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "does not match"):
            sepolia_e2e.validate_preflight(
                details,
                binding,
                artifacts,
                self.manifest,
            )

        artifacts = self.reinstall_artifacts(
            {"schema_version": 31, "deployment_instance_id": TX},
            {
                "replacement_mode": "current-schema-reinstall",
                "live_schema_version": 31,
                "previous_deployment_instance_id": TX,
                "live_module_hash": TX,
                "next": TX_B,
                "unexpected": True,
            },
        )
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "fields differ"):
            sepolia_e2e.validate_preflight(
                details,
                binding,
                artifacts,
                self.manifest,
            )

        artifacts = self.reinstall_artifacts()
        with self.assertRaisesRegex(
            sepolia_e2e.EvidenceError,
            "summary does not match",
        ):
            sepolia_e2e.validate_preflight(
                {
                    **details,
                    "replacement_mode": "obsolete-schema-reinstall",
                    "live_schema_version": 30,
                },
                binding,
                artifacts,
                self.manifest,
            )

        artifacts = self.reinstall_artifacts(
            {"schema_version": 31, "deployment_instance_id": TX},
            {
                "replacement_mode": "obsolete-schema-reinstall",
                "live_schema_version": 30,
                "previous_deployment_instance_id": TX,
                "live_module_hash": TX,
                "next": TX_B,
            },
        )
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "does not match"):
            sepolia_e2e.validate_preflight(
                details,
                binding,
                artifacts,
                self.manifest,
            )

    def test_preflight_rejects_required_artifact_hash_and_json_drift(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        artifacts = self.artifact("preflight")
        live_path = self.root / "artifacts/live-public-config.json"
        live_path.write_text('{"schema_version":30}\n', encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "sha256 does not match"):
            sepolia_e2e.validate_preflight(
                self.details("preflight"),
                binding,
                artifacts,
                self.manifest,
            )
        artifacts = self.artifact("preflight")
        live_path.write_text("{\n", encoding="utf-8")
        live_artifact = next(
            artifact
            for artifact in artifacts
            if artifact["kind"] == sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND
        )
        live_artifact["sha256"] = sepolia_e2e.digest(live_path)
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "cannot read JSON object"):
            sepolia_e2e.validate_preflight(
                self.details("preflight"),
                binding,
                artifacts,
                self.manifest,
            )

    def test_node_checker_output_is_accepted_by_manifest_validation(self) -> None:
        live = {"schema_version": 31, "deployment_instance_id": [17] * 32}
        live_path = self.root / "node-live-public-config.json"
        live_path.write_text(json.dumps(live), encoding="utf-8")
        status = {"module_hash": TX}
        status_path = self.root / "node-live-canister-status.json"
        status_path.write_text(json.dumps(status), encoding="utf-8")
        output = subprocess.run(
            [
                "node",
                str(MODULE_PATH.with_name("check-reinstall-instance.mjs")),
                str(self.profile),
                str(live_path),
                str(status_path),
            ],
            check=True,
            text=True,
            stdout=subprocess.PIPE,
        ).stdout
        check = json.loads(output)
        details = self.details("preflight")
        details["live_schema_version"] = 31
        details["previous_deployment_instance_id"] = TX
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        sepolia_e2e.validate_preflight(
            details,
            binding,
            self.reinstall_artifacts(live, check),
            self.manifest,
        )

    def test_later_record_revalidates_preflight_artifacts(self) -> None:
        sepolia_e2e.record(self.manifest, self.evidence("preflight"))
        (self.root / "artifacts/live-public-config.json").unlink()
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "does not exist"):
            sepolia_e2e.record(self.manifest, self.evidence("contracts"))

    def test_initialize_rejects_profile_instance_mismatch(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        details = self.details("initialize")
        details["deployment_instance_id"] = TX
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "differs"):
            sepolia_e2e.validate_initialize(details, binding)

    def test_deployment_identity_rejects_each_contract_tuple_drift(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        mutations = {
            "bridge_address": ADDRESS_C,
            "bsns_address": ADDRESS_C,
            "timelock_address": ADDRESS,
            "bridge_runtime_sha256": TX_B,
            "bsns_runtime_sha256": TX,
            "mint_signer": ADDRESS_B,
        }
        for field, value in mutations.items():
            with self.subTest(field=field):
                details = self.details("contracts")
                details[field] = value
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "differs"):
                    sepolia_e2e.validate_contracts(details, binding)

    def test_public_config_identity_rejects_each_shared_field_drift(self) -> None:
        binding = json.loads(self.manifest.read_text(encoding="utf-8"))["binding"]
        mutations = {
            "chain_id": 1,
            "bridge_address": ADDRESS_C,
            "timelock_address": ADDRESS,
            "expected_bridge_runtime_sha256": TX_B,
            "expected_bridge_signer": ADDRESS_B,
            "deployment_instance_id": TX,
        }
        for field, value in mutations.items():
            with self.subTest(field=field):
                details = self.details("initialize")
                details[field] = value
                with self.assertRaises(sepolia_e2e.EvidenceError):
                    sepolia_e2e.validate_initialize(details, binding)

    def test_upgrade_hash_drift_is_rejected(self) -> None:
        details = self.details("wallet_e2e")
        details["same_wasm_upgrade"]["after_state_sha256"] = H64_B
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "did not preserve canonical state"):
            sepolia_e2e.validate_wallet_e2e(details)

    def test_obsolete_local_upgrade_schema_is_rejected(self) -> None:
        self.manifest.unlink()
        local = json.loads(self.local.read_text(encoding="utf-8"))
        local["state_upgrade"]["before"]["status"]["schema_version"] = 28
        local["state_upgrade"]["after"]["status"]["schema_version"] = 28
        self.local.write_text(json.dumps(local), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "must use stable schema v31"):
            sepolia_e2e.initialize(self.manifest, self.local, self.profile)

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
