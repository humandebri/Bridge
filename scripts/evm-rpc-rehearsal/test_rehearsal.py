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
            },
        ),
        "deposit_mint": evidence(
            "deposit_mint",
            {"deposit_id": H32_A, "ledger_block_index": 1, "mint_transaction_hash": H32_B, "safe_block_number": 10, "safe_block_hash": H32_C},
        ),
        "withdrawal_release": evidence(
            "withdrawal_release",
            {"withdrawal_id": H32_A, "request_transaction_hash": H32_B, "acknowledge_transaction_hash": H32_D, "ledger_block_index": 2, "request_safe_block_number": 10, "request_safe_block_hash": H32_C, "acknowledgement_safe_block_number": 11, "acknowledgement_safe_block_hash": H32_A},
        ),
        "bad_fee_refund": evidence(
            "bad_fee_refund",
            {"withdrawal_id": H32_A, "request_transaction_hash": H32_B, "cancel_release_transaction_hash": H32_C, "refund_transaction_hash": H32_D, "old_ledger_fee": 10, "new_ledger_fee": 20, "amount_out_after_fee": 99, "minimum_amount_out": 100, "safe_block_number": 12, "safe_block_hash": H32_A},
        ),
        "canonical_receipt": evidence(
            "canonical_receipt",
            {"transaction_hash": H32_A, "receipt_block_number": 12, "receipt_block_hash": H32_B, "canonical_block_hash": H32_B, "confirmed_head_block_number": 13},
        ),
        "single_provider_failure": evidence(
            "single_provider_failure",
            {"configured_provider_count": 3, "agreeing_provider_count": 2, "bridge_operation_continued": True},
        ),
        "quorum_loss": evidence(
            "quorum_loss",
            {"configured_provider_count": 3, "agreeing_provider_count": 1, "fail_closed": True, "stop_reason": "RpcInconsistent", "ledger_call_performed": False},
        ),
        "nonce_known": evidence(
            "nonce_known",
            {"nonce_too_low_observed": True, "local_transaction_hash": H32_A, "provider_agreement": 2, "resulting_state": "Submitted"},
        ),
        "nonce_conflict": evidence(
            "nonce_conflict",
            {"nonce_too_low_observed": True, "local_transaction_hash": None, "resulting_stop_reason": "NonceConflict", "deposits_paused": True, "automatically_resigned": False},
        ),
        "final_pause": evidence(
            "final_pause",
            {"base_deposits_paused": True, "base_withdrawals_paused": True, "canister_deposits_paused": True, "safe_block_number": 14, "safe_block_hash": H32_D},
        ),
    }


def manifest(binding):
    return {
        "schema_version": 1,
        "kind": "base-sepolia-evm-rpc-canister-rehearsal",
        "state": "AWAITING_PREFLIGHT",
        "created_at": "2026-07-15T00:00:00Z",
        "updated_at": "2026-07-15T00:00:00Z",
        "source": {"revision": "a" * 40, "source_tree_sha256": "c" * 64, "worktree_status_sha256": "b" * 64},
        "binding": binding,
        "scenarios": {name: None for name in rehearsal.SCENARIOS},
        "complete": False,
        "guarantee_boundary": {
            "provider_operator_or_infrastructure_audited": False,
            "external_assumption": "The configured quorum returns the canonical Safe chain.",
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

    def test_self_reported_scenarios_cannot_derive_complete_state(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        for scenario, item in all_evidence(binding).items():
            rehearsal.record(value, item, scenario)
        self.assertEqual(value["state"], "AWAITING_RAW_ARTIFACT_VERIFICATION")
        self.assertFalse(value["complete"])

    def test_evidence_fails_closed_before_preflight_and_on_tamper(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        items = all_evidence(binding)
        with self.assertRaises(rehearsal.InvalidEvidence):
            rehearsal.record(value, items["deposit_mint"], "deposit_mint")
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
                "dfx",
                ["canister", "call", "aaaaa-aa", "get_status", "()", "--network", "ic", "--output", "json"],
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

            fake = root / "dfx"
            payload = json.dumps(item["details"], separators=(",", ":"))
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
                    "preflight", "bridge", str(output), "none", "--", "dfx", "canister", "call", "aaaaa-aa",
                    "get_public_config", "()", "--network", "ic", "--output", "json",
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
                    "preflight", "bridge", str(root / "artifacts" / "failed.json"), "none", "--", "dfx",
                    "canister", "call", "aaaaa-aa", "get_public_config", "()", "--network", "ic", "--output", "json",
                ],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                env=environment,
            )
            self.assertNotEqual(failed.returncode, 0)

    def test_fake_command_harness_completes_and_tamper_fails(self):
        binding = rehearsal.validate_config(config())
        value = manifest(binding)
        items = all_evidence(binding)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tool = root / "tool"
            for scenario, item in items.items():
                payload = json.dumps(item["details"], separators=(",", ":"))
                tool.write_text(
                    f"#!/bin/sh\nif [ \"$1\" = chain-id ]; then printf '84532\\n'; else printf '%s' '{payload}'; fi\n",
                    encoding="utf-8",
                )
                tool.chmod(0o755)
                for reference in item["artifacts"]:
                    kind = reference["kind"]
                    executable = root / ("cast" if kind == "base" else "dfx")
                    executable.write_bytes(tool.read_bytes())
                    executable.chmod(0o755)
                    output = root / reference["path"]
                    if kind == "base":
                        command = ["cast", "receipt", H32_A]
                    else:
                        canister = binding["ledger_canister_id"] if kind == "ledger" else binding["bridge_canister_id"]
                        method = {"ledger": "icrc3_get_blocks", "audit": "get_audit_events", "bridge": "get_bridge_status"}[kind]
                        command = ["dfx", "canister", "call", canister, method, "()", "--network", "ic", "--output", "json"]
                    with patch.dict(os.environ, {"PATH": f"{root}{os.pathsep}{os.environ.get('PATH', '')}"}):
                        rehearsal.capture_artifact(
                            value,
                            config(),
                            scenario,
                            kind,
                            output,
                            command,
                            0 if kind == "base" else None,
                        )
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
            self.assertTrue(value["complete"])
            manifest_path = root / "rpc-e2e.json"
            manifest_path.write_text(json.dumps(value), encoding="utf-8")
            verified = subprocess.run(
                ["python3", str(MODULE_PATH), "verify", str(manifest_path)],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            self.assertEqual(verified.returncode, 0, verified.stderr)

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
                "confirmed_head_block_number": "/confirmed_head_block_number",
            }},
            {"kind": "base", "path": "artifacts/canonical-receipt.json", "sha256": "0" * 64, "bindings": {
                "transaction_hash": "/transactionHash", "receipt_block_number": "/blockNumber",
                "receipt_block_hash": "/blockHash",
            }},
            {"kind": "base", "path": "artifacts/canonical-height.json", "sha256": "0" * 64, "bindings": {
                "canonical_block_hash": "/hash",
            }},
            {"kind": "base", "path": "artifacts/canonical-safe.json", "sha256": "0" * 64, "bindings": {
                "confirmed_head_block_number": "/number",
            }},
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dfx = root / "dfx"
            dfx.write_text(f"#!/bin/sh\nprintf '%s' '{json.dumps(item['details'], separators=(',', ':'))}'\n", encoding="utf-8")
            dfx.chmod(0o755)
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
                (item["artifacts"][0], ["dfx", "canister", "call", "aaaaa-aa", "get_deposit", "()", "--network", "ic", "--output", "json"], None),
                (item["artifacts"][1], ["cast", "receipt", H32_A], 0),
                (item["artifacts"][2], ["cast", "block", "12"], 0),
                (item["artifacts"][3], ["cast", "block", "safe"], 0),
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
