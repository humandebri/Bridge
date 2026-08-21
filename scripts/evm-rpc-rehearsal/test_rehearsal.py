#!/usr/bin/env python3
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("rehearsal.py")
SPEC = importlib.util.spec_from_file_location("evm_rpc_rehearsal", MODULE_PATH)
assert SPEC and SPEC.loader
rehearsal = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(rehearsal)


H32_A = "0x" + "11" * 32
H32_B = "0x" + "22" * 32
H32_C = "0x" + "33" * 32
H32_D = "0x" + "44" * 32
ADDRESS = "0x" + "aa" * 20
SIGNER = "0x" + "bb" * 20
SHA_A = "1" * 64
SHA_B = "2" * 64


def config():
    return {
        "schema_version": 1,
        "rehearsal_id": "base-sepolia-live-001",
        "test_only": True,
        "ic_network": "ic",
        "base_chain_id": 84532,
        "evm_rpc_canister_id": rehearsal.OFFICIAL_EVM_RPC_CANISTER_ID,
        "bridge_canister_id": "aaaaa-aa",
        "ledger_canister_id": "ryjl3-tyaaa-aaaaa-aaaba-cai",
        "index_canister_id": "qhbym-qaaaa-aaaaa-aaafq-cai",
        "bridge_contract": ADDRESS,
        "expected_bridge_signer": SIGNER,
        "bridge_canister_wasm_sha256": SHA_A,
        "bridge_runtime_bytecode_sha256": SHA_B,
        "rpc_urls": [
            "https://one.example/rpc",
            "https://two.example/rpc",
            "https://three.example/rpc",
        ],
    }


def evidence(scenario, details):
    kinds = sorted(rehearsal.REQUIRED_ARTIFACTS[scenario])
    artifacts = []
    for index, kind in enumerate(kinds):
        bindings = {
            field: f"/{field}"
            for field, required_kinds in rehearsal.CROSS_ARTIFACT_BINDINGS[scenario].items()
            if kind in required_kinds
        }
        if index == 0:
            bindings.update({field: f"/{field}" for field in details})
        if scenario == "preflight" and kind == "base":
            bindings["observed_chain_id"] = "/@transport/observed_chain_id"
        artifacts.append(
            {
                "kind": kind,
                "path": f"artifacts/{scenario}-{kind}.json",
                "sha256": "3" * 64,
                "bindings": bindings,
            }
        )
    if scenario == "preflight":
        base = next(artifact for artifact in artifacts if artifact["kind"] == "base")
        base["path"] = "artifacts/preflight-base-0.json"
        for provider_index in (1, 2):
            artifacts.append(
                {
                    **base,
                    "path": f"artifacts/preflight-base-{provider_index}.json",
                    "bindings": dict(base["bindings"]),
                }
            )
    if scenario in {"single_provider_failure", "quorum_loss"}:
        fault_fields = {"configured_provider_count", "required_provider_threshold", "injected_provider_failures", "fault_injection_reference"}
        for artifact in artifacts:
            for field in fault_fields:
                artifact["bindings"].pop(field, None)
            if artifact["kind"] == "fault":
                artifact["bindings"].update({
                    "configured_provider_count": "/configured_provider_count",
                    "required_provider_threshold": "/required_threshold",
                    "injected_provider_failures": "/failed_provider_count",
                    "fault_injection_reference": "/run_reference",
                })
    audit_methods = {
        "authorization_mint": "eth_getTransactionReceipt+multi_request",
        "withdrawal_release": "multi_request",
        "ledger_fee_guard": "multi_request",
        "canonical_receipt": "eth_getTransactionReceipt+multi_request",
        "authorization_expiry": "multi_request",
        "processed_event_mismatch": "multi_request",
    }
    audit_transaction_hash = next((value for key, value in reversed(list(details.items())) if "transaction_hash" in key and isinstance(value, str)), None)
    decisions = {
        "single_provider_failure": {
            "kind": "QuorumContinued", "operation": "request_deposit",
            "configured_provider_count": 3, "required_threshold": 2,
            "stop_reason": None, "ledger_call_performed": False,
            "bridge_operation_continued": True, "deposits_paused": False,
            "automatically_resigned": False, "transaction_hash": None,
        },
        "quorum_loss": {
            "kind": "QuorumLoss", "operation": "notify_withdrawal",
            "configured_provider_count": 3, "required_threshold": 2,
            "stop_reason": "RpcInconsistent", "ledger_call_performed": False,
            "bridge_operation_continued": False, "deposits_paused": False,
            "automatically_resigned": False, "transaction_hash": None,
        },
    }
    return {
        "schema_version": 1,
        "rehearsal_id": "base-sepolia-live-001",
        "scenario": scenario,
        "observed_at": "2026-07-15T00:00:00Z",
        "test_assets_only": True,
        "external_calls_performed": True,
        "through_evm_rpc_canister": True,
        "used_test_double": False,
        "ic_network": "ic",
        "base_chain_id": 84532,
        "evm_rpc_canister_id": rehearsal.OFFICIAL_EVM_RPC_CANISTER_ID,
        "bridge_canister_id": "aaaaa-aa",
        "request_sha256": SHA_A,
        "response_sha256": SHA_B,
        "result": "passed",
        "details": details,
        "canister_audit": (
            {
                "evm_rpc_canister_id": rehearsal.OFFICIAL_EVM_RPC_CANISTER_ID,
                "call_method": audit_methods.get(scenario, "multi_request"),
                "request_digest": "5" * 64,
                "quorum_response_digest": "6" * 64,
                "safe_block_number": next((value for key, value in details.items() if "safe_block_number" in key), 10),
                "safe_block_hash": next((value for key, value in details.items() if "safe_block_hash" in key), H32_C),
                "transaction_hash": audit_transaction_hash,
            }
            if scenario in rehearsal.RPC_AUDIT_SCENARIOS
            else None
        ),
        "canister_decision": decisions.get(scenario),
        "artifacts": artifacts,
    }


def all_evidence(binding):
    digests = [entry["url_sha256"] for entry in binding["rpc_endpoints"]]
    return {
        "preflight": evidence(
            "preflight",
            {
                "observed_chain_id": 84532,
                "observed_evm_rpc_canister_id": rehearsal.OFFICIAL_EVM_RPC_CANISTER_ID,
                "observed_bridge_contract": ADDRESS,
                "base_bridge_signer": SIGNER,
                "canister_chain_key_signer": SIGNER,
                "deposits_paused": True,
                "withdrawals_paused": True,
                "cycles_balance": 10_000_000_000,
                "base_sepolia_eth_balance_wei": 1,
                "configured_rpc_url_sha256": digests,
                "bridge_canister_module_sha256": SHA_A,
            },
        ),
        "authorization_mint": evidence(
            "authorization_mint",
            {"deposit_id": H32_A, "ledger_block_index": 1, "authorization_digest": H32_D, "mint_transaction_hash": H32_B, "safe_block_number": 10, "safe_block_hash": H32_C},
        ),
        "withdrawal_release": evidence(
            "withdrawal_release",
            {"withdrawal_id": H32_A, "request_transaction_hash": H32_B, "ledger_block_index": 2, "finalized_block_number": 10, "finalized_block_hash": H32_C},
        ),
        "ledger_fee_guard": evidence(
            "ledger_fee_guard",
            {"withdrawal_id": H32_A, "request_transaction_hash": H32_B, "observed_ledger_fee": 20, "charged_service_fee": 10, "stop_reason": "LedgerFeeExceedsServiceFee", "ledger_call_performed": False, "withdrawals_paused": True, "finalized_block_number": 12, "finalized_block_hash": H32_A},
        ),
        "canonical_receipt": evidence(
            "canonical_receipt",
            {"transaction_hash": H32_A, "receipt_block_number": 12, "receipt_block_hash": H32_B, "canonical_block_hash": H32_B, "finalized_checkpoint_block_number": 13},
        ),
        "single_provider_failure": evidence(
            "single_provider_failure",
            {"configured_provider_count": 3, "required_provider_threshold": 2, "injected_provider_failures": 1, "fault_injection_reference": "fault-one-provider", "threshold_satisfied": True, "bridge_operation_continued": True},
        ),
        "quorum_loss": evidence(
            "quorum_loss",
            {"configured_provider_count": 3, "required_provider_threshold": 2, "injected_provider_failures": 2, "fault_injection_reference": "fault-two-providers", "threshold_satisfied": False, "fail_closed": True, "stop_reason": "RpcInconsistent", "ledger_call_performed": False},
        ),
        "authorization_expiry": evidence(
            "authorization_expiry",
            {"deposit_id": H32_A, "authorization_digest": H32_B, "deadline": 100, "finalized_block_timestamp": 101, "finalized_block_hash": H32_C, "deposit_processed": False, "refund_ledger_block_index": 3},
        ),
        "processed_event_mismatch": evidence(
            "processed_event_mismatch",
            {"deposit_id": H32_A, "authorization_digest": H32_B, "deposit_processed": True, "exact_event_found": False, "stop_reason": "ProcessedEventMismatch", "deposits_paused": True, "refund_started": False},
        ),
        "final_pause": evidence(
            "final_pause",
            {"base_deposits_paused": True, "base_withdrawals_paused": True, "canister_deposits_paused": True, "safe_block_number": 14, "safe_block_hash": H32_D},
        ),
    }


def write_fault_artifact(item, scenario, output):
    details = item["details"]
    binding = rehearsal.validate_config(config())
    failed = list(range(details["injected_provider_failures"]))
    request = {
        "rehearsal_id": item["rehearsal_id"], "scenario": scenario,
        "run_reference": details["fault_injection_reference"],
        "provider_url_digests": [entry["url_sha256"] for entry in binding["rpc_endpoints"]],
        "failed_provider_indices": failed, "failure_rule": "connection-refused",
    }
    request_digest = rehearsal.hashlib.sha256(json.dumps(request, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    injector_output_digest = "8" * 64
    decision_timestamp_ns = int(rehearsal.datetime.fromisoformat(item["observed_at"].replace("Z", "+00:00")).timestamp() * 1_000_000_000)
    parsed = {
        "schema_version": 1,
        "rehearsal_id": item["rehearsal_id"],
        "scenario": scenario,
        "run_reference": details["fault_injection_reference"],
        "configured_provider_count": details["configured_provider_count"],
        "required_threshold": details["required_provider_threshold"],
        "failed_provider_count": details["injected_provider_failures"],
        "failed_provider_indices": failed,
        "provider_url_digests": request["provider_url_digests"],
        "failure_rule": "connection-refused",
        "started_at": item["observed_at"], "completed_at": item["observed_at"],
        "restored_provider_indices": failed,
        "injector_output_digest": injector_output_digest,
        "request_config_digest": request_digest,
        "decision_sequence": 7,
        "decision_timestamp_ns": decision_timestamp_ns,
        "decision_digest": rehearsal.hashlib.sha256(json.dumps(item["canister_decision"], sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
    }
    stdout = json.dumps(parsed, separators=(",", ":"))
    artifact = {
        "schema_version": 1, "scenario": scenario, "kind": "fault",
        "captured_at": item["observed_at"], "tool": "fault-injection-recorder",
        "argv": [request_digest, injector_output_digest], "exit_code": 0,
        "stdout_sha256": rehearsal.hashlib.sha256(stdout.encode()).hexdigest(),
        "stdout": stdout, "parsed": parsed, "transport": None,
    }
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(artifact, sort_keys=True), encoding="utf-8")


def manifest(binding):
    return {
        "schema_version": 2,
        "kind": "base-sepolia-evm-rpc-canister-rehearsal",
        "state": "AWAITING_PREFLIGHT",
        "created_at": "2026-07-15T00:00:00Z",
        "updated_at": "2026-07-15T00:00:00Z",
        "source": {"revision": "a" * 40, "source_tree_sha256": "c" * 64, "worktree_status_sha256": "b" * 64},
        "binding": binding,
        "scenarios": {name: None for name in rehearsal.SCENARIOS},
        "launch_ready": False,
        "extended_complete": False,
        "guarantee_boundary": {
            "provider_operator_or_infrastructure_audited": False,
            "external_assumption": "The configured quorum returns the canonical Finalized chain.",
        },
    }


class RehearsalTests(unittest.TestCase):
    def test_config_requires_official_canister_and_distinct_secret_free_urls(self):
        value = config()
        binding = rehearsal.validate_config(value)
        self.assertEqual(binding["evm_rpc_canister_id"], rehearsal.OFFICIAL_EVM_RPC_CANISTER_ID)
        self.assertNotIn("rpc_urls", binding)

        value = config()
        value["evm_rpc_canister_id"] = "aaaaa-aa"
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_config(value)

        value = config()
        value["rpc_urls"][1] = value["rpc_urls"][0]
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_config(value)

        value = config()
        value["rpc_urls"][1] = "https://user:secret@two.example/rpc"
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_config(value)

        value = config()
        value["rpc_urls"][1] = "https://two.example/v2/abcdefghijklmnopqrstuvwxyz012345"
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_config(value)

    def test_self_reported_scenarios_cannot_derive_launch_ready_state(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        for scenario, item in all_evidence(binding).items():
            rehearsal.record(value, item, scenario)
        self.assertEqual(value["state"], "AWAITING_RAW_ARTIFACT_VERIFICATION")
        self.assertFalse(value["launch_ready"])
        self.assertFalse(value["extended_complete"])

    def test_launch_and_extended_states_are_distinct(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        items = all_evidence(binding)
        for scenario in (
            "preflight",
            "authorization_mint",
            "withdrawal_release",
            "quorum_loss",
            "final_pause",
        ):
            value["scenarios"][scenario] = items[scenario]
        state, launch_ready, extended_complete = rehearsal.derive_state(value["scenarios"])
        self.assertEqual(state, "LAUNCH_READY")
        self.assertTrue(launch_ready)
        self.assertFalse(extended_complete)

        for scenario, item in items.items():
            value["scenarios"][scenario] = item
        state, launch_ready, extended_complete = rehearsal.derive_state(value["scenarios"])
        self.assertEqual(state, "EXTENDED_COMPLETE")
        self.assertTrue(launch_ready)
        self.assertTrue(extended_complete)

    def test_old_schema_and_post_pause_append_are_rejected(self):
        binding = rehearsal.validate_config(config())
        obsolete = manifest(binding)
        obsolete["schema_version"] = 1
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_manifest_envelope(obsolete)

        value = manifest(binding)
        items = all_evidence(binding)
        for scenario in (
            "preflight",
            "authorization_mint",
            "withdrawal_release",
            "quorum_loss",
            "final_pause",
        ):
            rehearsal.record(value, items[scenario], scenario)
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.record(
                value,
                items["canonical_receipt"],
                "canonical_receipt",
            )

    def test_boolean_claim_cannot_replace_canister_audit_or_module_binding(self):
        binding = rehearsal.validate_config(config())
        item = all_evidence(binding)["preflight"]
        item["canister_audit"] = None
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_common(item, binding, "preflight")

        item = all_evidence(binding)["preflight"]
        item["details"]["bridge_canister_module_sha256"] = "f" * 64
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_details("preflight", item["details"], binding)

        item = all_evidence(binding)["authorization_mint"]
        item["canister_audit"]["call_method"] = "obsolete_method"
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_common(item, binding, "authorization_mint")

        item = all_evidence(binding)["processed_event_mismatch"]
        item["canister_audit"] = None
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_common(item, binding, "processed_event_mismatch")

        item = all_evidence(binding)["processed_event_mismatch"]
        item["canister_decision"] = {}
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_common(item, binding, "processed_event_mismatch")

        item = all_evidence(binding)["single_provider_failure"]
        item["canister_decision"]["operation"] = "unrelated"
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_common(item, binding, "single_provider_failure")

    def test_evidence_fails_closed_before_preflight_and_on_tamper(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        items = all_evidence(binding)
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.record(value, items["authorization_mint"], "authorization_mint")
        rehearsal.record(value, items["preflight"], "preflight")
        bad = items["canonical_receipt"]
        bad["details"]["canonical_block_hash"] = H32_C
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.record(value, bad, "canonical_receipt")

    def test_init_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "exists.json"
            path.write_text("{}", encoding="utf-8")
            # The CLI checks this before deriving any mutable state.
            self.assertTrue(path.exists())

    def test_capture_rejects_obsolete_method_and_endpoint_overrides(self):
        binding = rehearsal.validate_config(config())
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_capture_command(
                "bridge",
                "icp",
                ["canister", "call", "aaaaa-aa", "get_status", "()", "-n", "ic", "--json"],
                binding,
            )
        for extra in (
            ["-n", "ic"],
            ["--output", "json"],
            ["--identity", "attacker"],
            ["--host=https://attacker.example"],
        ):
            with self.assertRaises(rehearsal.InvalidEvidence):
                rehearsal.validate_capture_command(
                    "bridge",
                    "icp",
                    ["canister", "call", "aaaaa-aa", "get_public_config", "()", "-n", "ic", "--json", *extra],
                    binding,
                )
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.validate_capture_command(
                "base",
                "cast",
                ["receipt", H32_A, "--rpc-url", "https://one.example/rpc"],
                binding,
            )
        value = manifest(binding)
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {"ETH_RPC_URL": "https://override.example"}):
            with self.assertRaises(rehearsal.InvalidEvidence):
                rehearsal.capture_artifact(
                    value,
                    config(),
                    "canonical_receipt",
                    "base",
                    Path(directory) / "base.json",
                    ["cast", "receipt", H32_A],
                    0,
                )

    def test_raw_artifact_is_required_and_capture_uses_fixed_command(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        item = all_evidence(binding)["preflight"]
        rehearsal.record(value, item, "preflight")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(rehearsal.InvalidEvidence):
                rehearsal.verify_manifest(value, root)

            fake = root / "icp"
            payload = json.dumps({**item["details"], "canister_audit": item["canister_audit"], "canister_decision": item["canister_decision"]}, separators=(",", ":"))
            fake.write_text(f"#!/bin/sh\nprintf '%s' '{payload}'\n", encoding="utf-8")
            fake.chmod(0o755)
            output = root / "artifacts" / "preflight-bridge.json"
            manifest_path = root / "rpc-e2e.json"
            config_path = root / "config.json"
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            config_path.write_text(json.dumps(config()), encoding="utf-8")
            environment = {**os.environ, "PATH": f"{root}{os.pathsep}{os.environ.get('PATH', '')}"}
            cli = subprocess.run(
                [
                    "python3", str(MODULE_PATH), "capture-artifact", str(manifest_path), str(config_path),
                    "preflight", "bridge", str(output), "none", "--", "icp", "canister", "call", "aaaaa-aa",
                    "get_public_config", "()", "-n", "ic", "--json",
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )
            self.assertEqual(cli.returncode, 0, cli.stderr)
            self.assertTrue(output.is_file())

            cast = root / "cast"
            cast.write_text(
                f"#!/bin/sh\nif [ \"$1\" = chain-id ]; then printf '84532\\n'; else printf '%s' '{payload}'; fi\n",
                encoding="utf-8",
            )
            cast.chmod(0o755)
            base_output = root / "artifacts" / "preflight-base.json"
            base_cli = subprocess.run(
                [
                    "python3", str(MODULE_PATH), "capture-artifact", str(manifest_path), str(config_path),
                    "preflight", "base", str(base_output), "0", "--", "cast", "receipt", H32_A,
                ],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env={**environment, "ETH_RPC_URL": "", "FOUNDRY_ETH_RPC_URL": "", "CAST_RPC_URL": ""},
            )
            self.assertEqual(base_cli.returncode, 0, base_cli.stderr)
            self.assertEqual(json.loads(base_output.read_text())["transport"]["provider_index"], 0)
            bridge_reference = next(reference for reference in item["artifacts"] if reference["kind"] == "bridge")
            bridge_reference["sha256"] = rehearsal.hashlib.sha256(output.read_bytes()).hexdigest()
            # A Base artifact remains mandatory; a single command capture cannot pass.
            with self.assertRaises(rehearsal.InvalidEvidence):
                rehearsal.verify_manifest(value, root)

            fake.write_text("#!/bin/sh\nexit 7\n", encoding="utf-8")
            failed = subprocess.run(
                [
                    "python3", str(MODULE_PATH), "capture-artifact", str(manifest_path), str(config_path),
                    "preflight", "bridge", str(root / "artifacts" / "failed.json"), "none", "--", "icp",
                    "canister", "call", "aaaaa-aa", "get_public_config", "()", "-n", "ic", "--json",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )
            self.assertNotEqual(failed.returncode, 0)

    def test_capture_fault_cli_binds_execution_and_rejects_wrong_restore(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path = root / "rpc-e2e.json"
            config_path = root / "config.json"
            output = root / "fault.json"
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            config_path.write_text(json.dumps(config()), encoding="utf-8")
            injector = root / "evm-rpc-fault-injector"
            decision = all_evidence(binding)["single_provider_failure"]["canister_decision"]
            injector.write_text(
                "#!/usr/bin/env python3\nimport json,time\nprint(json.dumps(" + repr({"schema_version": 1, "run_reference": "fault-one-provider", "applied_provider_indices": [0], "restored_provider_indices": [0], "result": "completed", "decision_sequence": 9, "decision_timestamp_ns": 0, "canister_decision": decision}) + ".copy() | {'decision_timestamp_ns': time.time_ns()}, separators=(',', ':')))\n",
                encoding="utf-8",
            )
            injector.chmod(0o755)
            command = [
                "python3", str(MODULE_PATH), "capture-fault", str(manifest_path), str(config_path),
                "single_provider_failure", str(output), "fault-one-provider", "--", "evm-rpc-fault-injector",
            ]
            environment = {**os.environ, "PATH": f"{root}{os.pathsep}{os.environ.get('PATH', '')}"}
            completed = subprocess.run(command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment)
            self.assertEqual(completed.returncode, 0, completed.stderr)
            artifact = json.loads(output.read_text())
            self.assertEqual(artifact["parsed"]["failed_provider_indices"], [0])
            self.assertEqual(artifact["parsed"]["restored_provider_indices"], [0])
            injector.write_text(
                "#!/usr/bin/env python3\nimport json,time\nprint(json.dumps(" + repr({"schema_version": 1, "run_reference": "fault-one-provider", "applied_provider_indices": [0], "restored_provider_indices": [], "result": "completed", "decision_sequence": 9, "decision_timestamp_ns": 0, "canister_decision": decision}) + ".copy() | {'decision_timestamp_ns': time.time_ns()}, separators=(',', ':')))\n",
                encoding="utf-8",
            )
            rejected = subprocess.run(command[:-4] + [str(root / "bad.json"), "fault-one-provider", "--", "evm-rpc-fault-injector"], text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=environment)
            self.assertNotEqual(rejected.returncode, 0)

    def test_fake_command_harness_completes_and_tamper_fails(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        items = all_evidence(binding)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tool = root / "tool"
            for scenario, item in items.items():
                fault_fields = {"configured_provider_count", "required_provider_threshold", "injected_provider_failures", "fault_injection_reference"}
                command_details = {key: value for key, value in item["details"].items() if scenario not in {"single_provider_failure", "quorum_loss"} or key not in fault_fields}
                audit_event = None
                if item["canister_decision"] is not None:
                    timestamp_ns = int(rehearsal.datetime.fromisoformat(item["observed_at"].replace("Z", "+00:00")).timestamp() * 1_000_000_000)
                    audit_event = {"sequence": 7, "timestamp_ns": timestamp_ns, "kind": {"EvmRpcDecision": item["canister_decision"]}}
                payload = json.dumps({**command_details, "canister_audit": item["canister_audit"], "audit_events": [audit_event] if audit_event else []}, separators=(",", ":"))
                tool.write_text(
                    f"#!/bin/sh\nif [ \"$1\" = chain-id ]; then printf '84532\\n'; else printf '%s' '{payload}'; fi\n",
                    encoding="utf-8",
                )
                tool.chmod(0o755)
                base_provider_index = 0
                for reference in item["artifacts"]:
                    kind = reference["kind"]
                    output = root / reference["path"]
                    if kind == "fault":
                        write_fault_artifact(item, scenario, output)
                        reference["sha256"] = rehearsal.hashlib.sha256(output.read_bytes()).hexdigest()
                        continue
                    executable = root / ("cast" if kind == "base" else "icp")
                    executable.write_bytes(tool.read_bytes())
                    executable.chmod(0o755)
                    if kind == "base":
                        command = ["cast", "receipt", H32_A]
                    elif kind == "module":
                        command = ["icp", "canister", "status", binding["bridge_canister_id"], "-n", "ic", "--public", "--json"]
                    else:
                        canister = binding["ledger_canister_id"] if kind == "ledger" else binding["bridge_canister_id"]
                        method = {"ledger": "icrc3_get_blocks", "audit": "get_audit_events", "bridge": "get_bridge_status"}[kind]
                        command = ["icp", "canister", "call", canister, method, "()", "-n", "ic", "--json"]
                    with patch.dict(os.environ, {"PATH": f"{root}{os.pathsep}{os.environ.get('PATH', '')}"}):
                        rehearsal.capture_artifact(
                            value,
                            config(),
                            scenario,
                            kind,
                            output,
                            command,
                            base_provider_index if kind == "base" else None,
                        )
                    if kind == "base":
                        base_provider_index += 1
                    reference["sha256"] = rehearsal.hashlib.sha256(output.read_bytes()).hexdigest()
                request_records = []
                response_records = []
                for reference in item["artifacts"]:
                    artifact = json.loads((root / reference["path"]).read_text(encoding="utf-8"))
                    request_records.append([artifact["tool"], *artifact["argv"], artifact["transport"]])
                    response_records.append(artifact["stdout"])
                item["request_sha256"] = rehearsal.hashlib.sha256(
                    json.dumps(request_records, separators=(",", ":")).encode()
                ).hexdigest()
                item["response_sha256"] = rehearsal.hashlib.sha256(
                    json.dumps(response_records, separators=(",", ":")).encode()
                ).hexdigest()
                rehearsal.record(value, item, scenario, root)
            rehearsal.verify_manifest(value, root)
            self.assertTrue(value["launch_ready"])
            self.assertTrue(value["extended_complete"])
            manifest_path = root / "rpc-e2e.json"
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            verified = subprocess.run(
                ["python3", str(MODULE_PATH), "verify", str(manifest_path)],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertEqual(verified.returncode, 0, verified.stderr)

            preflight_references = [
                reference
                for reference in items["preflight"]["artifacts"]
                if reference["kind"] == "base"
            ]
            duplicate_reference = preflight_references[2]
            duplicate_path = root / duplicate_reference["path"]
            original_preflight = duplicate_path.read_bytes()
            original_preflight_digest = duplicate_reference["sha256"]
            duplicate_provider = json.loads(original_preflight)
            duplicate_provider["transport"]["provider_index"] = 1
            duplicate_path.write_text(json.dumps(duplicate_provider), encoding="utf-8")
            duplicate_reference["sha256"] = rehearsal.hashlib.sha256(duplicate_path.read_bytes()).hexdigest()
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            duplicate_rejected = subprocess.run(
                ["python3", str(MODULE_PATH), "verify", str(manifest_path)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertNotEqual(duplicate_rejected.returncode, 0)
            duplicate_path.write_bytes(original_preflight)
            duplicate_reference["sha256"] = original_preflight_digest
            manifest_path.write_text(json.dumps(value), encoding="utf-8")

            base_reference = next(
                reference
                for reference in items["canonical_receipt"]["artifacts"]
                if reference["kind"] == "base"
            )
            base_path = root / base_reference["path"]
            original_base = base_path.read_bytes()
            original_digest = base_reference["sha256"]
            endpoint_tamper = json.loads(original_base)
            endpoint_tamper["transport"]["provider_index"] = 1
            base_path.write_text(json.dumps(endpoint_tamper), encoding="utf-8")
            base_reference["sha256"] = rehearsal.hashlib.sha256(base_path.read_bytes()).hexdigest()
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            endpoint_rejected = subprocess.run(
                ["python3", str(MODULE_PATH), "verify", str(manifest_path)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertNotEqual(endpoint_rejected.returncode, 0)
            base_path.write_bytes(original_base)
            base_reference["sha256"] = original_digest
            manifest_path.write_text(json.dumps(value), encoding="utf-8")

            tampered = root / items["canonical_receipt"]["artifacts"][0]["path"]
            tampered.write_text("{}\n", encoding="utf-8")
            with self.assertRaises(rehearsal.InvalidEvidence):
                rehearsal.verify_manifest(value, root)
            rejected = subprocess.run(
                ["python3", str(MODULE_PATH), "verify", str(manifest_path)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertNotEqual(rejected.returncode, 0)

    def test_canonical_receipt_binds_reachable_cast_and_candid_fields(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        preflight = all_evidence(binding)["preflight"]
        # This test targets artifact reachability, so preflight ordering is already established.
        value["scenarios"]["preflight"] = preflight
        item = all_evidence(binding)["canonical_receipt"]
        item["artifacts"] = [
            {"kind": "bridge", "path": "artifacts/canonical-bridge.json", "sha256": "0" * 64, "bindings": {
                "transaction_hash": "/transaction_hash", "receipt_block_number": "/receipt_block_number",
                "receipt_block_hash": "/receipt_block_hash", "canonical_block_hash": "/canonical_block_hash",
                "finalized_checkpoint_block_number": "/finalized_checkpoint_block_number",
            }},
            {"kind": "base", "path": "artifacts/canonical-receipt.json", "sha256": "0" * 64, "bindings": {
                "transaction_hash": "/transactionHash", "receipt_block_number": "/blockNumber",
                "receipt_block_hash": "/blockHash",
            }},
            {"kind": "base", "path": "artifacts/canonical-height.json", "sha256": "0" * 64, "bindings": {
                "canonical_block_hash": "/hash",
            }},
            {"kind": "base", "path": "artifacts/canonical-safe.json", "sha256": "0" * 64, "bindings": {
                "finalized_checkpoint_block_number": "/number",
            }},
            {"kind": "audit", "path": "artifacts/canonical-audit.json", "sha256": "0" * 64, "bindings": {}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            icp = root / "icp"
            icp.write_text(f"#!/bin/sh\nprintf '%s' '{json.dumps({**item['details'], 'canister_audit': item['canister_audit'], 'canister_decision': item['canister_decision']}, separators=(',', ':'))}'\n", encoding="utf-8")
            icp.chmod(0o755)
            cast = root / "cast"
            cast.write_text(
                "#!/bin/sh\n"
                "if [ \"$1\" = chain-id ]; then printf '84532\\n'; "
                f"elif [ \"$1\" = receipt ]; then printf '%s' '{json.dumps({'transactionHash': H32_A, 'blockNumber': 12, 'blockHash': H32_B}, separators=(',', ':'))}'; "
                f"elif [ \"$2\" = 12 ]; then printf '%s' '{json.dumps({'number': 12, 'hash': H32_B}, separators=(',', ':'))}'; "
                f"else printf '%s' '{json.dumps({'number': 13, 'hash': H32_C}, separators=(',', ':'))}'; fi\n",
                encoding="utf-8",
            )
            cast.chmod(0o755)
            commands = [
                (item["artifacts"][0], ["icp", "canister", "call", "aaaaa-aa", "get_deposit", "()", "-n", "ic", "--json"], None),
                (item["artifacts"][1], ["cast", "receipt", H32_A], 0),
                (item["artifacts"][2], ["cast", "block", "12"], 0),
                (item["artifacts"][3], ["cast", "block", "safe"], 0),
                (item["artifacts"][4], ["icp", "canister", "call", "aaaaa-aa", "get_audit_events", "()", "-n", "ic", "--json"], None),
            ]
            clean = {**os.environ, "PATH": f"{root}{os.pathsep}{os.environ.get('PATH', '')}", "ETH_RPC_URL": "", "FOUNDRY_ETH_RPC_URL": "", "CAST_RPC_URL": ""}
            with patch.dict(os.environ, clean, clear=True):
                for reference, command, provider in commands:
                    output = root / reference["path"]
                    rehearsal.capture_artifact(value, config(), "canonical_receipt", reference["kind"], output, command, provider)
                    reference["sha256"] = rehearsal.hashlib.sha256(output.read_bytes()).hexdigest()
            request_records, response_records = [], []
            for reference in item["artifacts"]:
                artifact = json.loads((root / reference["path"]).read_text())
                request_records.append([artifact["tool"], *artifact["argv"], artifact["transport"]])
                response_records.append(artifact["stdout"])
            item["request_sha256"] = rehearsal.hashlib.sha256(json.dumps(request_records, separators=(",", ":")).encode()).hexdigest()
            item["response_sha256"] = rehearsal.hashlib.sha256(json.dumps(response_records, separators=(",", ":")).encode()).hexdigest()
            rehearsal.record(value, item, "canonical_receipt", root)


if __name__ == "__main__":
    unittest.main()
