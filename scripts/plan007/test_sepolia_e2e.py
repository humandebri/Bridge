#!/usr/bin/env python3
from __future__ import annotations

import copy
import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock


MODULE_PATH = Path(__file__).with_name("sepolia_e2e.py")
REPO_ROOT = MODULE_PATH.parents[2]
SPEC = importlib.util.spec_from_file_location("sepolia_e2e", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
sepolia_e2e = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(sepolia_e2e)
ORIGINAL_VERIFY_RPC_REHEARSAL_MANIFEST = sepolia_e2e.verify_rpc_rehearsal_manifest

SOURCE = "a" * 40
H64 = "a" * 64
H64_B = "b" * 64
H64_C = "c" * 64
TX = "0x" + H64
TX_B = "0x" + H64_B
TX_C = "0x" + H64_C
ADDRESS = "0x7936c3587902907db4918a2466cbc225fdc090dc"
ADDRESS_B = "0x3a6e18a8bcae2b0e94383e0b5c2117107fcc53cd"
ADDRESS_C = "0x5ffbd4b328a2d5688c4100183acc7246f7e5a5d3"
ADDRESS_D = "0x801ddc785d2e79da8366cd282a1e5a9bed2492b5"
BRIDGE_CANISTER_ID = "rlhjx-iyaaa-aaaaf-qcnyq-cai"
LEDGER_CANISTER_ID = "3jkp5-oyaaa-aaaaj-azwqa-cai"
INDEX_CANISTER_ID = "qzre3-3iaaa-aaaai-aqmsa-cai"
PROFILE_INSTANCE = "0x067b39c87fb0fae8fa5c161fc5e04641adbea230b571e11496e323e30b5bcda1"
MINIMUM_WITHDRAWAL_ID = "0x" + "7b" * 32
RPC_PROVIDER_URLS_SHA256 = "0x" + "05" * 32
OBSERVED_AT = "2026-08-31T00:00:00Z"


class SepoliaEvidenceTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.manifest = self.root / "manifest.json"
        self.local = self.root / "local-e2e.json"
        self.profile = self.root / "frontend-profile.json"
        self.artifact_root = self.root / "artifacts"
        self.artifact_root.mkdir()
        upgrade_state = {
            "owner_sequence": "1",
            "status": {
                "schema_version": 35,
                "counts": {
                    "pending_ledger_operations": "0",
                    "reserved_deposit_mint_operations": "1",
                },
                "settlement_scheduler": {},
            },
            "runtime_binding": {
                "deployment_instance_id": list(bytes.fromhex(PROFILE_INSTANCE[2:])),
                "minimum_withdrawal_id": [0x7B] * 32,
                "base_chain_id": 84532,
                "bridge_contract": [1] * 20,
                "expected_bridge_runtime_sha256": [1] * 32,
                "timelock_contract": [3] * 20,
                "expected_bridge_signer": [4] * 20,
                "ledger_canister_id": LEDGER_CANISTER_ID,
                "index_canister_id": INDEX_CANISTER_ID,
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "rpc_provider_urls_sha256": [5] * 32,
                "schema_version": 35,
                "operational_config_sha256": [6] * 32,
            },
            "operational_config": {
                "deposit_rate_limit_window_seconds": 60,
                "deposit_rate_limit_global": 30,
                "deposit_rate_limit_per_principal": 3,
                "notification_rate_limit_window_seconds": 600,
                "notification_rate_limit_global": 60,
                "notification_ingestion_rate_limit_global": 30,
                "settlement_rate_limit_window_seconds": 3600,
                "settlement_rate_limit_global": 60,
                "settlement_rate_limit_per_principal": 30,
                "settlement_rate_limit_per_record": 3,
                "settlement_retry_interval_seconds": 60,
            },
            "deposits": [{"deposit_id": [9] * 32}],
            "withdrawals": [],
            "audit_events": {},
            "activation_status": {
                "pending_timelock_operation": [],
                "deposits_paused": False,
            },
            "storage_integrity": "ok",
        }
        self.local.write_text(
            json.dumps(
                {
                    "schema_version": 8,
                    "environment_mode": "short-delay-test-only",
                    "activation_timelock_delay_seconds": 300,
                    "stable_schema_version": 35,
                    "record_wire_version": 30,
                    "deployment_instance_id": PROFILE_INSTANCE,
                    "created_at": OBSERVED_AT,
                    "source_commit": SOURCE,
                    "bridge_wasm_sha256": H64,
                    "candid_sha256": H64_B,
                    "bridge_runtime_template_sha256": TX,
                    "bsns_runtime_template_sha256": TX_B,
                    "bridge_abi_sha256": H64,
                    "bsns_abi_sha256": H64_B,
                    "ledger_release": "ledger-suite-icrc-2026-03-09",
                    "ledger_wasm_sha256": "354dd6ecfdc72b5409805b31dea22c9db11df6e14095a5a68924eb63535e6d8a",
                    "index_wasm_sha256": "dab6808d0dfc06e5e88336d0c3d3e45e5448c6e36c2a781f3e9e09bd450f528c",
                    "state_upgrade": {
                        "verified": True,
                        "before": upgrade_state,
                        "after": copy.deepcopy(upgrade_state),
                    },
                    "tests": {
                        "full_local_ci": "passed",
                        "real_frontend_e2e": "passed",
                        "canister_activation": "passed",
                        "timelock_delay_enforced": "passed",
                        "state_upgrade": "passed",
                    },
                }
            )
            + "\n",
            encoding="utf-8",
        )
        self.profile.write_text(
            json.dumps(
                {
                    "environment": "sepolia-staging",
                    "testOnly": True,
                    "chainId": 84532,
                    "evmRpcCanisterId": "7hfb6-caaaa-aaaar-qadga-cai",
                    "rpcProviderUrlsSha256": RPC_PROVIDER_URLS_SHA256,
                    "environmentMode": "short-delay-test-only",
                    "activationTimelockDelaySeconds": 300,
                    "bridgeCanisterId": BRIDGE_CANISTER_ID,
                    "ledgerCanisterId": LEDGER_CANISTER_ID,
                    "indexCanisterId": INDEX_CANISTER_ID,
                    "deploymentInstanceId": PROFILE_INSTANCE,
                    "minimumWithdrawalId": MINIMUM_WITHDRAWAL_ID,
                    "bridgeAddress": ADDRESS,
                    "bsnsAddress": ADDRESS_B,
                    "timelockAddress": ADDRESS_C,
                    "bridgeRuntimeHash": TX,
                    "bsnsRuntimeHash": TX_B,
                    "expected_bridge_signer": ADDRESS_D,
                }
            )
            + "\n",
            encoding="utf-8",
        )
        sepolia_e2e.initialize(self.manifest, self.local, self.profile)
        self.rpc_verifier_patch = mock.patch.object(
            sepolia_e2e, "verify_rpc_rehearsal_manifest"
        )
        self.rpc_verifier = self.rpc_verifier_patch.start()

    def tearDown(self) -> None:
        self.rpc_verifier_patch.stop()
        self.temporary.cleanup()

    def write_artifact(self, name: str, kind: str, value: object) -> dict[str, str]:
        path = self.artifact_root / f"{name}.json"
        path.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")
        return {
            "path": f"artifacts/{path.name}",
            "sha256": sepolia_e2e.digest(path),
            "kind": kind,
        }

    def generic_artifact(
        self, stage: str, details: dict[str, object]
    ) -> list[dict[str, str]]:
        kind = sepolia_e2e.STAGE_RECEIPT_KINDS[stage]
        capture_kind = sepolia_e2e.STAGE_RAW_CAPTURE_KINDS[stage]
        stdout = json.dumps(details, sort_keys=True, separators=(",", ":"))
        capture = self.write_artifact(
            f"{stage}-raw-capture",
            capture_kind,
            {
                "schema_version": 1,
                "kind": capture_kind,
                "stage": stage,
                "observed_at": OBSERVED_AT,
                "source_commit": SOURCE,
                "tool": "approved-stage-recorder",
                "argv": ["approved-stage-recorder", stage],
                "exit_code": 0,
                "stdout": stdout,
                "stdout_sha256": sepolia_e2e.hashlib.sha256(stdout.encode()).hexdigest(),
            },
        )
        receipt = self.write_artifact(
            stage,
            kind,
            {
                "schema_version": 8,
                "kind": kind,
                "stage": stage,
                "observed_at": OBSERVED_AT,
                "source_commit": SOURCE,
                "details_sha256": sepolia_e2e.object_digest(details),
                "capture_sha256": capture["sha256"],
            },
        )
        return [receipt, capture]

    def rpc_artifact(self) -> list[dict[str, str]]:
        binding = sepolia_e2e.load_object(self.manifest)["binding"]
        value = {
            "state": "EXTENDED_COMPLETE",
            "launch_ready": True,
            "extended_complete": True,
            "scenarios": {scenario: {} for scenario in sepolia_e2e.RPC_SCENARIOS},
            "binding": {
                "base_chain_id": binding["chain_id"],
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "bridge_canister_id": binding["bridge_canister_id"],
                "ledger_canister_id": binding["ledger_canister_id"],
                "index_canister_id": binding["index_canister_id"],
                "bridge_contract": binding["bridge_address"],
                "expected_bridge_signer": binding["expected_bridge_signer"],
                "bridge_canister_wasm_sha256": binding["bridge_wasm_sha256"],
                "bridge_runtime_bytecode_sha256": binding["bridge_runtime_sha256"][2:],
                "rpc_endpoints": [
                    {"url_sha256": digest}
                    for digest in (H64, H64_B, H64_C)
                ],
            },
        }
        return [
            self.write_artifact(
                "rpc-rehearsal-manifest",
                sepolia_e2e.RPC_REHEARSAL_MANIFEST_ARTIFACT_KIND,
                value,
            )
        ]

    def live_artifacts(self, details: dict[str, object]) -> list[dict[str, str]]:
        schedule = {
            "schema_version": 1,
            "kind": sepolia_e2e.REACTIVATION_SCHEDULE_RECEIPT_ARTIFACT_KIND,
            "chain_id": 84532,
            "timelock_address": ADDRESS_C,
            "operation_id": details["reactivation_operation_id"],
            "transaction_hash": details["reactivation_schedule_transaction_hash"],
            "success": True,
            "block_number": 10,
            "block_hash": TX_B,
            "block_timestamp": 1_000,
            "delay_seconds": 300,
            "ready_timestamp": 1_300,
        }
        execute = {
            "schema_version": 1,
            "kind": sepolia_e2e.REACTIVATION_EXECUTE_RECEIPT_ARTIFACT_KIND,
            "chain_id": 84532,
            "timelock_address": ADDRESS_C,
            "operation_id": details["reactivation_operation_id"],
            "transaction_hash": details["reactivation_execute_transaction_hash"],
            "success": True,
            "block_number": 11,
            "block_hash": TX_C,
            "block_timestamp": 1_300,
        }
        monitoring = {
            "artifact_schema_version": 1,
            "kind": sepolia_e2e.STAGING_MONITORING_RECEIPT_ARTIFACT_KIND,
            "observed_at": OBSERVED_AT,
            **{
                field: details[field]
                for field in sepolia_e2e.LIVE_MONITOR_FIELDS
            },
        }
        artifacts = [
            self.write_artifact(
                "reactivation-schedule",
                sepolia_e2e.REACTIVATION_SCHEDULE_RECEIPT_ARTIFACT_KIND,
                schedule,
            ),
            self.write_artifact(
                "reactivation-execute",
                sepolia_e2e.REACTIVATION_EXECUTE_RECEIPT_ARTIFACT_KIND,
                execute,
            ),
            self.write_artifact(
                "staging-monitoring",
                sepolia_e2e.STAGING_MONITORING_RECEIPT_ARTIFACT_KIND,
                monitoring,
            ),
        ]
        details["monitoring_receipt_sha256"] = artifacts[-1]["sha256"]
        return artifacts

    def bootstrap_artifacts(self) -> list[dict[str, str]]:
        descriptors = []
        for filename, kind in (
            (
                "reinstall-decision-2026-08-27.json",
                sepolia_e2e.HISTORICAL_REINSTALL_DECISION_ARTIFACT_KIND,
            ),
            (
                "fresh-stack-2026-08-28.json",
                sepolia_e2e.HISTORICAL_FRESH_STACK_ARTIFACT_KIND,
            ),
        ):
            source = REPO_ROOT / "deployments/sepolia-staging/evidence" / filename
            target = self.root / filename
            shutil.copyfile(source, target)
            descriptors.append(
                {"path": filename, "sha256": sepolia_e2e.digest(target), "kind": kind}
            )
        return descriptors

    def counts(self) -> dict[str, int]:
        return {field: 0 for field in sepolia_e2e.UPGRADE_STATE_COUNT_FIELDS}

    def preflight_artifacts(self) -> list[dict[str, str]]:
        values = [
            (
                sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND,
                {
                    "schema_version": 35,
                    "deployment_instance_id": PROFILE_INSTANCE,
                    "rpc_provider_urls_sha256": [5] * 32,
                },
            ),
            (
                sepolia_e2e.UPGRADE_INSTANCE_CHECK_ARTIFACT_KIND,
                {
                    "replacement_mode": "current-schema-upgrade",
                    "live_schema_version": 35,
                    "previous_deployment_instance_id": PROFILE_INSTANCE,
                    "live_module_hash": TX,
                    "next": PROFILE_INSTANCE,
                },
            ),
            (
                sepolia_e2e.WITHDRAWAL_BOUNDARY_ARTIFACT_KIND,
                {
                    "schema_version": 1,
                    "kind": sepolia_e2e.WITHDRAWAL_BOUNDARY_ARTIFACT_KIND,
                    "observed_at": OBSERVED_AT,
                    "chain_id": 84532,
                    "bridge_address": ADDRESS,
                    "finalized_checkpoint_block_number": 10,
                    "finalized_checkpoint_block_hash": TX,
                    "withdrawals_paused": False,
                    "minimum_withdrawal_id": MINIMUM_WITHDRAWAL_ID,
                    "providers": [
                        {
                            "provider_url_sha256": provider,
                            "finalized_head_block_number": 10 + index,
                            "checkpoint_block_number": 10,
                            "checkpoint_block_hash": TX,
                            "withdrawals_paused": False,
                            "minimum_withdrawal_id": MINIMUM_WITHDRAWAL_ID,
                        }
                        for index, provider in enumerate((H64, H64_B, H64_C))
                    ],
                },
            ),
            (sepolia_e2e.LIVE_BRIDGE_STATUS_ARTIFACT_KIND, self.counts()),
            (
                sepolia_e2e.LIVE_ACTIVATION_STATUS_ARTIFACT_KIND,
                {"pending_timelock_operations": 0},
            ),
            (sepolia_e2e.LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND, {"result": "ok"}),
            (sepolia_e2e.LIVE_LEDGER_BALANCE_ARTIFACT_KIND, {"balance_raw": 1}),
            (
                sepolia_e2e.LIVE_LEDGER_METADATA_ARTIFACT_KIND,
                {
                    "schema_version": 1,
                    "kind": sepolia_e2e.LIVE_LEDGER_METADATA_ARTIFACT_KIND,
                    "ledger_canister_id": LEDGER_CANISTER_ID,
                    "index_canister_id": INDEX_CANISTER_ID,
                    "index_ledger_id": LEDGER_CANISTER_ID,
                    "symbol": "TICRC1",
                    "decimals": 8,
                    "fee": 10_000,
                },
            ),
            (
                sepolia_e2e.LIVE_CANISTER_STATUS_ARTIFACT_KIND,
                {
                    "module_hash": TX,
                    "controller_principals": ["aaaaa-aa"],
                    "cycles_balance": 100,
                },
            ),
        ]
        return [self.write_artifact(kind, kind, value) for kind, value in values]

    def details(self, stage: str) -> dict[str, object]:
        if stage == "bootstrap_attestation":
            return {
                "historical_reinstall_completed": True,
                "historical_reinstall_resumable": False,
                "future_update_mode": "current-schema-upgrade",
                "bridge_canister_id": BRIDGE_CANISTER_ID,
                "deployment_instance_id": PROFILE_INSTANCE,
                "stable_schema_version": 35,
            }
        if stage == "preflight":
            return {
                "chain_id": 84532,
                "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                "bridge_canister_id": BRIDGE_CANISTER_ID,
                "ledger_canister_id": LEDGER_CANISTER_ID,
                "index_canister_id": INDEX_CANISTER_ID,
                "ledger_symbol": "TICRC1",
                "ledger_decimals": 8,
                "ledger_fee": 10_000,
                "index_ledger_id": LEDGER_CANISTER_ID,
                "controller_principals": ["aaaaa-aa"],
                "cycles_balance": 100,
                "base_deposits_paused": False,
                "base_withdrawals_paused": False,
                "canister_deposits_paused": False,
                "configured_rpc_url_sha256": [H64, H64_B, H64_C],
                "replacement_mode": "current-schema-upgrade",
                "live_schema_version": 35,
                "previous_deployment_instance_id": PROFILE_INSTANCE,
                "minimum_withdrawal_id": MINIMUM_WITHDRAWAL_ID,
            }
        if stage == "current_schema_upgrade":
            return {
                "install_mode": "upgrade",
                "module_sha256": H64,
                "candid_sha256": H64_B,
                "cycles_balance_before": 100,
                "cycles_balance_after": 90,
                "controller_principals_before": ["aaaaa-aa"],
                "controller_principals_after": ["aaaaa-aa"],
                "state_counts_before": self.counts(),
                "state_counts_after": self.counts(),
                "bridge_canister_id_before": BRIDGE_CANISTER_ID,
                "bridge_canister_id_after": BRIDGE_CANISTER_ID,
                "schema_version_before": 35,
                "schema_version_after": 35,
                "record_wire_version_before": 30,
                "record_wire_version_after": 30,
                "deployment_instance_id_before": PROFILE_INSTANCE,
                "deployment_instance_id_after": PROFILE_INSTANCE,
                "minimum_withdrawal_id_before": MINIMUM_WITHDRAWAL_ID,
                "minimum_withdrawal_id_after": MINIMUM_WITHDRAWAL_ID,
                "storage_integrity_after": "ok",
            }
        if stage == "post_upgrade_binding":
            return {
                "canister": {
                    "schema_version": 35,
                    "record_wire_version": 30,
                    "deployment_instance_id": PROFILE_INSTANCE,
                    "minimum_withdrawal_id": MINIMUM_WITHDRAWAL_ID,
                    "chain_id": 84532,
                    "ledger_canister_id": LEDGER_CANISTER_ID,
                    "index_canister_id": INDEX_CANISTER_ID,
                    "evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
                    "rpc_provider_urls_sha256": RPC_PROVIDER_URLS_SHA256,
                    "expected_bridge_signer": ADDRESS_D,
                    "bridge_address": ADDRESS,
                    "timelock_address": ADDRESS_C,
                    "expected_bridge_runtime_sha256": TX,
                    "module_sha256": H64,
                    "candid_sha256": H64_B,
                    "governance_operator": ADDRESS_B,
                    "canister_deposits_paused": False,
                    "storage_integrity": "ok",
                },
                "contracts": {
                    "bridge_address": ADDRESS,
                    "bsns_address": ADDRESS_B,
                    "timelock_address": ADDRESS_C,
                    "bridge_runtime_template_sha256": TX,
                    "bsns_runtime_template_sha256": TX_B,
                    "bridge_runtime_sha256": TX,
                    "bsns_runtime_sha256": TX_B,
                    "deployment_block": 1,
                    "deployment_transaction_hashes": [TX],
                    "mint_signer": ADDRESS_D,
                    "governance_operator": ADDRESS_B,
                    "deployer_roles_zero": True,
                },
            }
        if stage == "frontend_publish":
            profile_hash = sepolia_e2e.load_object(self.manifest)["binding"]["frontend_profile_sha256"]
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
                "withdrawal_transaction_hash": TX_C,
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
                "finalized_block_hash": TX_C,
                "completed": True,
            }
            return {
                "chrome_version": "1",
                "wallet_versions": {
                    "Plug": "1", "OISY": "1", "MetaMask": "1", "Rabby": "1", "WalletConnect": "1"
                },
                "deposits": [{**flow, "wallet": "Plug"}, {**flow, "wallet": "OISY"}],
                "withdrawals": [{**flow, "wallet": "MetaMask"}, {**flow, "wallet": "Rabby"}],
                "walletconnect": {
                    "connected": True,
                    "rejection_safe": True,
                    "account_change_safe": True,
                    "chain_change_safe": True,
                    "csp_clean": True,
                },
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
                "same_wasm_upgrade": {
                    "before_state_sha256": H64,
                    "after_state_sha256": H64,
                    "storage_integrity": "ok",
                    "verified": True,
                },
            }
        if stage == "refund_rehearsal":
            return {
                "deposit_id": TX,
                "authorization_digest": TX_B,
                "deadline": 1_000,
                "at_deadline_result": "NotClaimable",
                "after_deadline_timestamp": 1_001,
                "deposit_processed": False,
                "refund_ledger_block": 7,
                "final_state": "Refunded",
                "finalized_block_number": 10,
                "finalized_block_hash": TX_C,
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
        if stage == "live_acceptance":
            binding = sepolia_e2e.load_object(self.manifest)["binding"]
            return {
                "initial_activation_operation_id": TX,
                "reactivation_operation_id": TX_B,
                "reactivation_schedule_transaction_hash": TX_B,
                "reactivation_execute_transaction_hash": TX_C,
                "reactivation_delay_seconds": 300,
                "base_deposits_paused": False,
                "base_withdrawals_paused": False,
                "canister_deposits_paused": False,
                "pending_governance_operations": 0,
                "pending_timelock_operations": 0,
                "pending_deposits": 0,
                "pending_withdrawals": 0,
                "pending_reconciliation_jobs": 0,
                "pending_ledger_operations": 0,
                "reserved_deposit_mint_operations": 0,
                "reserved_deposit_mint_amount": 0,
                "unpaid_withdrawal_count": 0,
                "unpaid_withdrawal_amount_out": 0,
                "providers_restored": True,
                "settlement_scheduler_healthy": True,
                "storage_integrity": "ok",
                "reserve_sufficient": True,
                "schema_version": 35,
                "record_wire_version": 30,
                "deployment_instance_id": PROFILE_INSTANCE,
                "minimum_withdrawal_id": MINIMUM_WITHDRAWAL_ID,
                "bridge_canister_id": BRIDGE_CANISTER_ID,
                "module_sha256": H64,
                "candid_sha256": H64_B,
                "frontend_profile_sha256": binding["frontend_profile_sha256"],
                "bridge_runtime_sha256": TX,
                "mint_authorization_ttl_seconds": 600,
                "solidity_max_authorization_horizon_seconds": 900,
                "old_stack_excluded": True,
                "monitoring_receipt_sha256": H64_C,
                "finalized_block_number": 11,
                "finalized_block_hash": TX_C,
            }
        raise AssertionError(stage)

    def artifacts(
        self, stage: str, details: dict[str, object]
    ) -> list[dict[str, str]]:
        if stage == "bootstrap_attestation":
            return self.bootstrap_artifacts()
        if stage == "preflight":
            return self.preflight_artifacts()
        if stage == "rpc_rehearsal":
            artifacts = self.rpc_artifact()
            details["manifest_sha256"] = artifacts[0]["sha256"]
            return artifacts
        if stage == "live_acceptance":
            return self.live_artifacts(details)
        return self.generic_artifact(stage, details)

    def evidence(self, stage: str, *, details: dict[str, object] | None = None) -> Path:
        path = self.root / f"{stage}-evidence.json"
        recorded_details = details if details is not None else self.details(stage)
        path.write_text(
            json.dumps(
                {
                    "schema_version": 8,
                    "stage": stage,
                    "observed_at": OBSERVED_AT,
                    "source_commit": SOURCE,
                    "artifacts": self.artifacts(stage, recorded_details),
                    "details": recorded_details,
                }
            )
            + "\n",
            encoding="utf-8",
        )
        return path

    def record_through(self, final_stage: str) -> None:
        for stage in sepolia_e2e.STAGES:
            sepolia_e2e.record(self.manifest, self.evidence(stage))
            if stage == final_stage:
                return
        raise AssertionError(final_stage)

    def test_complete_manifest_reaches_short_delay_live(self) -> None:
        self.record_through("live_acceptance")
        manifest = sepolia_e2e.load_object(self.manifest)
        sepolia_e2e.validate_manifest(
            manifest,
            self.manifest,
            require_complete=True,
            verify_files=True,
        )
        self.assertEqual(manifest["state"], "SHORT_DELAY_LIVE")
        self.assertTrue(manifest["complete"])
        self.rpc_verifier.assert_called()

    def test_stage_order_has_no_reinstall_or_top_level_final_pause(self) -> None:
        self.assertEqual(
            sepolia_e2e.STAGES,
            (
                "bootstrap_attestation",
                "preflight",
                "current_schema_upgrade",
                "post_upgrade_binding",
                "frontend_publish",
                "smoke_e2e",
                "wallet_e2e",
                "refund_rehearsal",
                "rpc_rehearsal",
                "live_acceptance",
            ),
        )
        self.assertNotIn("reinstall", sepolia_e2e.STAGES)
        self.assertNotIn("final_pause", sepolia_e2e.STAGES)
        self.assertIn("final_pause", sepolia_e2e.RPC_SCENARIOS)

    def test_out_of_order_and_v7_stage_evidence_are_rejected(self) -> None:
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "expected stage bootstrap_attestation"):
            sepolia_e2e.record(self.manifest, self.evidence("preflight"))
        evidence = json.loads(self.evidence("bootstrap_attestation").read_text())
        evidence["schema_version"] = 7
        path = self.root / "v7-stage.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "wrong schema"):
            sepolia_e2e.record(self.manifest, path)

    def test_v7_local_evidence_is_not_migrated(self) -> None:
        local = json.loads(self.local.read_text())
        local["schema_version"] = 7
        self.local.write_text(json.dumps(local), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "schema v8"):
            sepolia_e2e.initialize(
                self.root / "v7-manifest.json", self.local, self.profile
            )

    def test_local_evidence_inside_the_repository_is_rejected(self) -> None:
        with self.assertRaisesRegex(
            sepolia_e2e.EvidenceError,
            "local promotion evidence must be generated outside the repository",
        ):
            sepolia_e2e.initialize(
                self.root / "inside-repo-manifest.json",
                self.local,
                self.profile,
                repo_root=self.root,
            )

    def test_local_evidence_requires_current_schema_wire_and_complete_upgrade_state(self) -> None:
        for mutate, message in (
            (
                lambda value: value.__setitem__("record_wire_version", 29),
                "stable schema or record wire",
            ),
            (
                lambda value: (
                    value["state_upgrade"]["before"].pop("activation_status"),
                    value["state_upgrade"]["after"].pop("activation_status"),
                ),
                "missing required upgrade state",
            ),
        ):
            with self.subTest(message=message):
                local = json.loads(self.local.read_text())
                mutate(local)
                candidate = self.root / f"invalid-{message.split()[0]}.json"
                candidate.write_text(json.dumps(local), encoding="utf-8")
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, message):
                    sepolia_e2e.initialize(
                        self.root / f"manifest-{message.split()[0]}.json",
                        candidate,
                        self.profile,
                    )

    def test_bootstrap_attestation_is_non_resumable_and_hash_bound(self) -> None:
        invalid = self.details("bootstrap_attestation")
        invalid["historical_reinstall_resumable"] = True
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "must be false"):
            sepolia_e2e.record(
                self.manifest,
                self.evidence("bootstrap_attestation", details=invalid),
            )
        evidence = json.loads(self.evidence("bootstrap_attestation").read_text())
        evidence["artifacts"][0]["sha256"] = H64
        tampered = self.root / "tampered-bootstrap.json"
        tampered.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(
            sepolia_e2e.EvidenceError, "sha256 does not match|pinned historical"
        ):
            sepolia_e2e.record(self.manifest, tampered)

    def test_upgrade_rejects_reinstall_instance_and_preflight_drift(self) -> None:
        self.record_through("preflight")
        for field, value, message in (
            ("install_mode", "reinstall", "only a current-schema"),
            ("bridge_canister_id_after", "aaaaa-aa", "changed the Bridge Canister"),
            ("record_wire_version_after", 29, "record wire version"),
            ("deployment_instance_id_after", TX_C, "changed the deployment instance"),
            ("cycles_balance_before", 101, "preflight Canister status"),
        ):
            with self.subTest(field=field):
                details = self.details("current_schema_upgrade")
                details[field] = value
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, message):
                    sepolia_e2e.record(
                        self.manifest,
                        self.evidence("current_schema_upgrade", details=details),
                    )

    def test_preflight_rejects_live_ledger_fee_drift(self) -> None:
        self.record_through("bootstrap_attestation")
        evidence = json.loads(self.evidence("preflight").read_text())
        metadata_descriptor = next(
            artifact
            for artifact in evidence["artifacts"]
            if artifact["kind"] == sepolia_e2e.LIVE_LEDGER_METADATA_ARTIFACT_KIND
        )
        metadata_path = self.root / metadata_descriptor["path"]
        metadata = json.loads(metadata_path.read_text())
        metadata["fee"] = 100_000
        metadata_path.write_text(json.dumps(metadata), encoding="utf-8")
        metadata_descriptor["sha256"] = sepolia_e2e.digest(metadata_path)
        path = self.root / "fee-drift-preflight.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "Ledger metadata"):
            sepolia_e2e.record(self.manifest, path)

    def test_preflight_rejects_runtime_rpc_provider_digest_drift(self) -> None:
        self.record_through("bootstrap_attestation")
        evidence = json.loads(self.evidence("preflight").read_text())
        descriptor = next(
            artifact
            for artifact in evidence["artifacts"]
            if artifact["kind"] == sepolia_e2e.LIVE_PUBLIC_CONFIG_ARTIFACT_KIND
        )
        artifact_path = self.root / descriptor["path"]
        runtime = json.loads(artifact_path.read_text())
        runtime["rpc_provider_urls_sha256"] = [6] * 32
        artifact_path.write_text(json.dumps(runtime), encoding="utf-8")
        descriptor["sha256"] = sepolia_e2e.digest(artifact_path)
        path = self.root / "rpc-provider-digest-drift.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "RPC provider digest"):
            sepolia_e2e.record(self.manifest, path)

    def test_post_upgrade_rejects_runtime_rpc_provider_digest_drift(self) -> None:
        self.record_through("current_schema_upgrade")
        details = self.details("post_upgrade_binding")
        details["canister"]["rpc_provider_urls_sha256"] = TX_C
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "RPC provider digest"):
            sepolia_e2e.record(
                self.manifest,
                self.evidence("post_upgrade_binding", details=details),
            )

    def test_stage_receipt_must_bind_the_recorded_details(self) -> None:
        self.record_through("preflight")
        evidence = json.loads(self.evidence("current_schema_upgrade").read_text())
        receipt_path = self.root / evidence["artifacts"][0]["path"]
        receipt = json.loads(receipt_path.read_text())
        receipt["details_sha256"] = H64_C
        receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
        evidence["artifacts"][0]["sha256"] = sepolia_e2e.digest(receipt_path)
        evidence_path = self.root / "unbound-upgrade.json"
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "recorded details"):
            sepolia_e2e.record(self.manifest, evidence_path)

        evidence = json.loads(self.evidence("current_schema_upgrade").read_text())
        receipt_descriptor = next(
            artifact
            for artifact in evidence["artifacts"]
            if artifact["kind"] == "current_schema_upgrade-receipt"
        )
        capture_descriptor = next(
            artifact
            for artifact in evidence["artifacts"]
            if artifact["kind"] == "current_schema_upgrade-raw-capture"
        )
        capture_path = self.root / capture_descriptor["path"]
        capture = json.loads(capture_path.read_text())
        capture["stdout"] = "{}"
        capture["stdout_sha256"] = sepolia_e2e.hashlib.sha256(b"{}").hexdigest()
        capture_path.write_text(json.dumps(capture), encoding="utf-8")
        capture_descriptor["sha256"] = sepolia_e2e.digest(capture_path)
        receipt_path = self.root / receipt_descriptor["path"]
        receipt = json.loads(receipt_path.read_text())
        receipt["capture_sha256"] = capture_descriptor["sha256"]
        receipt_path.write_text(json.dumps(receipt), encoding="utf-8")
        receipt_descriptor["sha256"] = sepolia_e2e.digest(receipt_path)
        evidence_path = self.root / "unbound-raw-upgrade.json"
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "does not reproduce"):
            sepolia_e2e.record(self.manifest, evidence_path)

    def test_refund_rehearsal_enforces_strict_deadline_and_unprocessed_state(self) -> None:
        valid = self.details("refund_rehearsal")
        sepolia_e2e.validate_refund_rehearsal(valid)
        for field, value, message in (
            ("after_deadline_timestamp", 1_000, "strictly after"),
            ("deposit_processed", True, "must be false"),
            ("at_deadline_result", "Refunded", "equal to the deadline"),
        ):
            with self.subTest(field=field):
                invalid = copy.deepcopy(valid)
                invalid[field] = value
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, message):
                    sepolia_e2e.validate_refund_rehearsal(invalid)

    def test_live_acceptance_fails_closed_on_pause_pending_provider_and_identity_drift(self) -> None:
        binding = sepolia_e2e.load_object(self.manifest)["binding"]
        valid = self.details("live_acceptance")
        sepolia_e2e.validate_live_acceptance(valid, binding)
        cases = (
            ("base_deposits_paused", True),
            ("pending_deposits", 1),
            ("pending_ledger_operations", 1),
            ("reserved_deposit_mint_amount", 1),
            ("unpaid_withdrawal_count", 1),
            ("providers_restored", False),
            ("deployment_instance_id", TX_C),
            ("module_sha256", H64_C),
            ("reactivation_operation_id", TX),
        )
        for field, value in cases:
            with self.subTest(field=field):
                invalid = copy.deepcopy(valid)
                invalid[field] = value
                with self.assertRaises(sepolia_e2e.EvidenceError):
                    sepolia_e2e.validate_live_acceptance(invalid, binding)

    def test_live_acceptance_binds_timelock_and_monitoring_receipts(self) -> None:
        self.record_through("rpc_rehearsal")
        evidence = json.loads(self.evidence("live_acceptance").read_text())
        schedule_path = self.root / evidence["artifacts"][0]["path"]
        schedule = json.loads(schedule_path.read_text())
        schedule["ready_timestamp"] -= 1
        schedule_path.write_text(json.dumps(schedule), encoding="utf-8")
        evidence["artifacts"][0]["sha256"] = sepolia_e2e.digest(schedule_path)
        evidence_path = self.root / "bad-ready-time.json"
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "exact five-minute"):
            sepolia_e2e.record(self.manifest, evidence_path)

        evidence = json.loads(self.evidence("live_acceptance").read_text())
        monitor_path = self.root / evidence["artifacts"][2]["path"]
        monitor = json.loads(monitor_path.read_text())
        monitor["providers_restored"] = False
        monitor_path.write_text(json.dumps(monitor), encoding="utf-8")
        evidence["artifacts"][2]["sha256"] = sepolia_e2e.digest(monitor_path)
        evidence["details"]["monitoring_receipt_sha256"] = evidence["artifacts"][2]["sha256"]
        evidence_path = self.root / "bad-monitor.json"
        evidence_path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "providers_restored"):
            sepolia_e2e.record(self.manifest, evidence_path)

    def test_rpc_rehearsal_hash_and_verifier_are_mandatory(self) -> None:
        self.record_through("refund_rehearsal")
        evidence = json.loads(self.evidence("rpc_rehearsal").read_text())
        evidence["details"]["manifest_sha256"] = H64_C
        path = self.root / "rpc-hash-drift.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "summary digest"):
            sepolia_e2e.record(self.manifest, path)

        self.rpc_verifier.side_effect = sepolia_e2e.EvidenceError("invalid raw RPC evidence")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "invalid raw RPC"):
            sepolia_e2e.record(self.manifest, self.evidence("rpc_rehearsal"))

    def test_rpc_rehearsal_rejects_provider_order_drift(self) -> None:
        self.record_through("refund_rehearsal")
        evidence = json.loads(self.evidence("rpc_rehearsal").read_text())
        artifact_path = self.root / evidence["artifacts"][0]["path"]
        rehearsal = json.loads(artifact_path.read_text())
        rehearsal["binding"]["rpc_endpoints"].reverse()
        artifact_path.write_text(json.dumps(rehearsal), encoding="utf-8")
        artifact_sha256 = sepolia_e2e.digest(artifact_path)
        evidence["artifacts"][0]["sha256"] = artifact_sha256
        evidence["details"]["manifest_sha256"] = artifact_sha256
        path = self.root / "rpc-provider-order-drift.json"
        path.write_text(json.dumps(evidence), encoding="utf-8")
        with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "provider order"):
            sepolia_e2e.record(self.manifest, path)

    def test_rpc_verifier_command_fails_closed(self) -> None:
        self.rpc_verifier_patch.stop()
        try:
            with mock.patch.object(
                sepolia_e2e.subprocess,
                "run",
                return_value=mock.Mock(returncode=1, stderr="bad evidence", stdout=""),
            ) as run:
                with self.assertRaisesRegex(sepolia_e2e.EvidenceError, "bad evidence"):
                    ORIGINAL_VERIFY_RPC_REHEARSAL_MANIFEST(self.root / "rpc.json")
                self.assertEqual(run.call_args.args[0][-2], "verify")
        finally:
            self.rpc_verifier_patch = mock.patch.object(
                sepolia_e2e, "verify_rpc_rehearsal_manifest"
            )
            self.rpc_verifier = self.rpc_verifier_patch.start()

    def test_checked_in_schema_matches_python_v8_contract(self) -> None:
        schema_path = MODULE_PATH.parents[2] / "deployments/sepolia-staging/sepolia-e2e.schema.json"
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        self.assertEqual(schema["properties"]["schema_version"]["const"], 8)
        self.assertEqual(
            set(schema["properties"]["stages"]["required"]),
            set(sepolia_e2e.STAGES),
        )
        expected_states = {
            f"AWAITING_{stage.upper()}" for stage in sepolia_e2e.STAGES
        } | {"SHORT_DELAY_LIVE"}
        self.assertEqual(
            set(schema["properties"]["state"]["enum"]), expected_states
        )
        binding = sepolia_e2e.load_object(self.manifest)["binding"]
        self.assertEqual(
            set(schema["properties"]["binding"]["required"]), set(binding)
        )


if __name__ == "__main__":
    unittest.main()
