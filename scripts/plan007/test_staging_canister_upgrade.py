#!/usr/bin/env python3
"""Regression tests for the fail-closed staging RPC replacement driver."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from source_resolution import source_path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT_FILES = (
    "scripts/plan007/staging-canister-upgrade.sh",
    "scripts/plan007/staging_canister_upgrade.py",
    "scripts/plan007/candid_values.py",
    "scripts/plan007/read-public-canister-metadata.mjs",
)
POLICY_PATH = "deployments/sepolia-staging/rpc-provider-replacement-policy.json"


class StagingUpgradeDriverTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.repo = self.base / "repo"
        self.state = self.base / "state"
        self.repo.mkdir()
        self.state.mkdir()
        for relative in SCRIPT_FILES + (POLICY_PATH,):
            destination = self.repo / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            source = (
                source_path(relative) if relative in SCRIPT_FILES else ROOT / relative
            )
            shutil.copy2(source, destination)
        (self.repo / "canister/bridge-canister").mkdir(parents=True)
        self.did = self.repo / "canister/bridge-canister/bridge.did"
        self.did.write_text("service : {}\n", encoding="utf-8")
        policy = self.policy()
        self.valid_policy = policy
        staging = self.repo / "deployments/sepolia-staging"
        (staging / "evidence").mkdir(parents=True, exist_ok=True)
        (staging / "frontend-profile.json").write_text(json.dumps({
            "environment": policy["environment"],
            "bridgeCanisterId": policy["canister_id"],
            "deploymentInstanceId": policy["deployment_instance_id"],
            "chainId": policy["base_chain_id"],
            "evmRpcCanisterId": policy["evm_rpc_canister_id"],
            "icHost": "https://icp-api.io",
            "baseRpcUrl": policy["after_rpc_urls"][0],
            "baseHistoryRpcUrls": policy["after_rpc_urls"][1:],
            "rpcProviderUrlsSha256": "0x" + policy["after_rpc_urls_sha256"],
            "minimumWithdrawalId": "0x" + "01" * 32,
        }), encoding="utf-8")
        self.wasm = self.base / "reviewed.wasm"
        self.wasm.write_bytes(b"reviewed staging wasm")
        self.after_module = hashlib.sha256(self.wasm.read_bytes()).hexdigest()
        self.bin = self.base / "bin"
        self.bin.mkdir()
        self.install_record = self.state / "install.json"
        self.write_tools()
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "Test")
        self.git("add", ".")
        self.git("commit", "-qm", "code")
        source = self.git("rev-parse", "HEAD").stdout.strip()
        local_e2e = {
            "source_commit": source,
            "bridge_wasm_sha256": self.after_module,
            "candid_sha256": hashlib.sha256(self.did.read_bytes()).hexdigest(),
        }
        (staging / "evidence/local-e2e.json").write_text(json.dumps(local_e2e), encoding="utf-8")
        self.git("add", "deployments/sepolia-staging/evidence/local-e2e.json")
        self.git("commit", "-qm", "evidence")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def git(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", *args], cwd=self.repo, text=True, capture_output=True, check=True)

    def policy(self) -> dict[str, object]:
        return json.loads((self.repo / POLICY_PATH).read_text(encoding="utf-8"))

    def migration_source(self, candid_metadata: str) -> dict[str, object]:
        return next(
            source
            for source in self.policy()["migration"]["source_states"]
            if source["candid_metadata"] == candid_metadata
        )

    def write_executable(self, name: str, source: str) -> None:
        path = self.bin / name
        path.write_text(source, encoding="utf-8")
        path.chmod(0o755)

    def write_tools(self) -> None:
        self.write_executable("cast", "#!/bin/sh\n[ \"${MOCK_CHAIN_FAIL:-0}\" = 0 ] || exit 1\necho \"${MOCK_CHAIN_ID:-84532}\"\n")
        self.write_executable("didc", "#!/bin/sh\n[ \"${MOCK_DIDC_FAIL:-0}\" = 0 ] || exit 1\n")
        self.write_executable("node", r'''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
state = pathlib.Path(os.environ["MOCK_STATE"])
if args and args[0].endswith("capture-withdrawal-boundary.mjs"):
    print(json.dumps({
        "schema_version": 1,
        "kind": "withdrawal-admission-boundary",
        "minimum_withdrawal_id": os.environ.get("MOCK_CAPTURE_BOUNDARY", "0x" + "01" * 32),
        "providers": [{"provider_url_sha256": "11" * 32}, {"provider_url_sha256": "22" * 32}],
    }))
    raise SystemExit(0)
if len(args) != 4 or args[1] != "https://icp-api.io" or args[3] != "candid:service":
    raise SystemExit(2)
if os.environ.get("MOCK_PUBLIC_METADATA_FAIL") == "1":
    print("certified metadata lookup failed", file=sys.stderr)
    raise SystemExit(1)
if "MOCK_PUBLIC_METADATA_JSON" in os.environ:
    print(os.environ["MOCK_PUBLIC_METADATA_JSON"])
    raise SystemExit(0)
if os.environ.get("MOCK_METADATA_MISSING") == "1" and not (state / "metadata-repaired").exists():
    print(json.dumps({"status": "absent"}))
    raise SystemExit(0)
print(json.dumps({"status": "present", "value": pathlib.Path(os.environ["MOCK_DID"]).read_text()}))
''')
        self.write_executable("ic-wasm", r'''#!/usr/bin/env python3
import os, pathlib, sys
args = sys.argv[1:]
if args[-1] == "metadata":
    sections = ["icp:private kinic:deployment"] if os.environ.get("MOCK_CANDID_SECTION_MISSING") == "1" else ["icp:public candid:service", "icp:private kinic:deployment"]
    print("\n".join(sections))
elif args[-2:] == ["metadata", "candid:service"]:
    print(pathlib.Path(os.environ["MOCK_DID"]).read_text())
elif args[-2:] == ["metadata", "kinic:deployment"]:
    print(os.environ.get("MOCK_CANDID_DEPLOYMENT", "test-deployment"))
else:
    raise SystemExit(2)
''')
        self.write_executable("icp", r"""#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
state = pathlib.Path(os.environ["MOCK_STATE"])
policy = json.loads(pathlib.Path(os.environ["MOCK_POLICY"]).read_text())
after_module = os.environ["MOCK_AFTER_MODULE"]
applied = (state / "applied").exists() or os.environ.get("MOCK_ALREADY_APPLIED") == "1"
digest = policy["after_rpc_urls_sha256"] if applied else policy["before_rpc_urls_sha256"]
if not applied:
    digest = os.environ.get("MOCK_DIGEST", digest)
module = after_module if (state / "metadata-repaired").exists() else os.environ.get(
    "MOCK_MODULE", after_module if applied else policy["before_module_sha256"]
)
def blob(value): return ''.join('\\' + value[i:i+2] for i in range(0, len(value), 2))
if args[:2] == ["canister", "install"]:
    (state / "install.json").write_text(json.dumps(args))
    if os.environ.get("MOCK_INSTALL_FAIL") == "1": raise SystemExit(1)
    (state / "applied").touch(); (state / "metadata-repaired").touch(); raise SystemExit(0)
if args[:2] == ["canister", "metadata"]:
    name = args[3]
    if name == "kinic:deployment":
        if os.environ.get("MOCK_PRIVATE_METADATA_FAIL") == "1": raise SystemExit(1)
        print(json.dumps({"value": os.environ.get("MOCK_LIVE_DEPLOYMENT", "test-deployment")})); raise SystemExit(0)
    raise SystemExit(2)
if args[:2] == ["canister", "status"]:
    if "--id-only" in args: print(os.environ.get("MOCK_CANISTER_ID", policy["canister_id"])); raise SystemExit(0)
    print(json.dumps({"module_hash": module})); raise SystemExit(0)
if args[:2] != ["canister", "call"]: raise SystemExit(2)
method = args[3]
if method == "start_storage_validation":
    if os.environ.get("MOCK_VALIDATION_FAIL") == "1":
        candid = 'variant { Err = variant { StorageFailure } }'
    else:
        (state / "validation-started").touch()
        candid = 'variant { Ok = record { complete = false; phase = "deposits"; scanned_rows = 0 : nat64 } }'
elif method == "continue_storage_validation":
    if os.environ.get("MOCK_VALIDATION_FAIL") == "1":
        candid = 'variant { Err = variant { StateChanged } }'
    else:
        candid = 'variant { Ok = record { complete = true; phase = "complete"; scanned_rows = 1 : nat64 } }'
elif method == "get_public_config":
    schema = "33" if (state / "applied").exists() else os.environ.get("MOCK_SCHEMA", "33")
    instance = os.environ.get("MOCK_INSTANCE", policy["deployment_instance_id"])[2:]
    chain = os.environ.get("MOCK_PUBLIC_CHAIN", str(policy["base_chain_id"]))
    evm = os.environ.get("MOCK_EVM_CANISTER", policy["evm_rpc_canister_id"])
    boundary = os.environ.get("MOCK_BOUNDARY", "01" * 32)
    boundary_field = f'; minimum_withdrawal_id = blob "{blob(boundary)}"' if int(schema) >= 33 else ''
    candid = f'''record {{ schema_version = {schema} : nat16; deployment_instance_id = blob "{blob(instance)}"; base_chain_id = {chain} : nat64; evm_rpc_canister_id = principal "{evm}"; rpc_provider_urls_sha256 = blob "{blob(digest)}"{boundary_field} }}'''
elif method == "get_bridge_status":
    counts = dict(policy["status_counts"])
    if os.environ.get("MOCK_COUNT_DRIFT") == "1" or (applied and os.environ.get("MOCK_POST_COUNT_DRIFT") == "1"): counts["deposits"] += 1
    candid = 'record { ' + '; '.join(f'{key} = {value} : nat64' for key, value in counts.items()) + ' }'
elif method == "storage_integrity_check":
    candid = 'variant { Err = variant { StorageFailure } }' if os.environ.get("MOCK_INTEGRITY_FAIL") == "1" else 'variant { Ok = "ok" }'
else: raise SystemExit(2)
print(json.dumps({"response_candid": candid}))
""")

    def run_driver(self, *extra: str, **changes: str) -> subprocess.CompletedProcess[str]:
        evidence = self.base / "result.json"
        environment = os.environ.copy()
        environment.update({
            "PATH": f"{self.bin}{os.pathsep}{environment['PATH']}",
            "BRIDGE_STAGING_IDENTITY": "reviewed-controller",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "MOCK_STATE": str(self.state),
            "MOCK_POLICY": str(self.repo / POLICY_PATH),
            "MOCK_AFTER_MODULE": self.after_module,
            "MOCK_DID": str(self.did),
        })
        environment.update(changes)
        return subprocess.run([
            "bash", str(self.repo / SCRIPT_FILES[0]), "--wasm", str(self.wasm),
            "--evidence", str(evidence), *extra,
        ], cwd=self.repo, env=environment, text=True, capture_output=True, check=False)

    def test_preflight_is_read_only(self) -> None:
        result = self.run_driver()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("preflight-passed", result.stdout)
        self.assertFalse(self.install_record.exists())
        self.assertFalse((self.base / "result.json").exists())
        self.assertFalse((self.repo / "scripts/plan007/__pycache__").exists())

    def test_preflight_rejects_storage_validation_failure_before_install(self) -> None:
        result = self.run_driver("--execute", MOCK_VALIDATION_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.install_record.exists())

    def test_execute_uses_explicit_wasm_and_writes_verified_evidence(self) -> None:
        result = self.run_driver("--execute")
        self.assertEqual(result.returncode, 0, result.stderr)
        command = json.loads(self.install_record.read_text())
        self.assertNotIn("deploy", command)
        self.assertEqual(command[command.index("--wasm") + 1], str(self.wasm.resolve()))
        candid_args = command[command.index("--args") + 1]
        self.assertIn("expected_status_counts = record", candid_args)
        self.assertIn("status_counts_guard_version = 1 : nat8", candid_args)
        for field, value in self.policy()["status_counts"].items():
            annotation = "nat" if field == "reserved_deposit_mint_amount" else "nat64"
            self.assertIn(f"{field} = {value} : {annotation}", candid_args)
        evidence = json.loads((self.base / "result.json").read_text())
        self.assertEqual(evidence["schema_version"], 2)
        self.assertEqual(evidence["result"], "upgraded")
        self.assertIsNone(evidence["boundary_capture"])
        self.assertEqual(evidence["minimum_withdrawal_id"], "0x" + "01" * 32)
        self.assertEqual(evidence["before"]["status_counts"], evidence["after"]["status_counts"])

    def test_already_applied_is_idempotent(self) -> None:
        preflight = self.run_driver(MOCK_ALREADY_APPLIED="1")
        self.assertEqual(preflight.returncode, 0, preflight.stderr)
        self.assertIn("preflight-passed", preflight.stdout)
        self.assertFalse(self.install_record.exists())
        self.assertFalse((self.base / "result.json").exists())

        result = self.run_driver("--execute", MOCK_ALREADY_APPLIED="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.install_record.exists())
        self.assertEqual(json.loads((self.base / "result.json").read_text())["result"], "already-applied")

    def test_migration_flag_is_rejected_outside_a_v32_source(self) -> None:
        result = self.run_driver("--migrate-v32-to-v33", "--execute")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires a reviewed v32 staging source state", result.stderr)
        self.assertFalse(self.install_record.exists())

    def test_known_missing_metadata_requires_explicit_migration(self) -> None:
        source = self.migration_source("absent")
        missing_module = str(source["module_sha256"])
        after_digest = str(source["rpc_provider_urls_sha256"])
        rejected = self.run_driver(
            MOCK_SCHEMA="32", MOCK_MODULE=missing_module, MOCK_METADATA_MISSING="1", MOCK_DIGEST=after_digest
        )
        self.assertNotEqual(rejected.returncode, 0)
        self.assertFalse(self.install_record.exists())

        preflight = self.run_driver(
            "--migrate-v32-to-v33",
            MOCK_SCHEMA="32", MOCK_MODULE=missing_module, MOCK_METADATA_MISSING="1", MOCK_DIGEST=after_digest,
        )
        self.assertEqual(preflight.returncode, 0, preflight.stderr)
        self.assertIn("v32-to-v33-preflight-passed", preflight.stdout)
        self.assertFalse(self.install_record.exists())

        repaired = self.run_driver(
            "--migrate-v32-to-v33", "--execute",
            MOCK_SCHEMA="32", MOCK_MODULE=missing_module, MOCK_METADATA_MISSING="1", MOCK_DIGEST=after_digest,
        )
        self.assertEqual(repaired.returncode, 0, repaired.stderr)
        evidence = json.loads((self.base / "result.json").read_text())
        self.assertEqual(evidence["result"], "migrated-and-rpc-replaced")
        self.assertEqual(evidence["before"]["module_sha256"], missing_module)
        self.assertEqual(evidence["after"]["module_sha256"], self.after_module)
        self.assertEqual(evidence["live_candid_sha256_after"], evidence["candid_sha256"])
        self.assertEqual(evidence["boundary_capture"]["minimum_withdrawal_id"], "0x" + "01" * 32)
        self.assertEqual(evidence["minimum_withdrawal_id"], "0x" + "01" * 32)

    def test_known_v32_source_state_requires_the_explicit_migration_flag(self) -> None:
        source = self.migration_source("present")
        before_module = str(source["module_sha256"])
        before_digest = str(source["rpc_provider_urls_sha256"])
        rejected = self.run_driver(
            MOCK_SCHEMA="32", MOCK_MODULE=before_module, MOCK_DIGEST=before_digest,
        )
        self.assertNotEqual(rejected.returncode, 0)
        self.assertFalse(self.install_record.exists())

        preflight = self.run_driver(
            "--migrate-v32-to-v33",
            MOCK_SCHEMA="32", MOCK_MODULE=before_module, MOCK_DIGEST=before_digest,
        )
        self.assertEqual(preflight.returncode, 0, preflight.stderr)
        self.assertIn("v32-to-v33-preflight-passed", preflight.stdout)
        self.assertFalse(self.install_record.exists())

        migrated = self.run_driver(
            "--migrate-v32-to-v33", "--execute",
            MOCK_SCHEMA="32", MOCK_MODULE=before_module, MOCK_DIGEST=before_digest,
        )
        self.assertEqual(migrated.returncode, 0, migrated.stderr)
        evidence = json.loads((self.base / "result.json").read_text())
        self.assertEqual(evidence["result"], "migrated-and-rpc-replaced")
        self.assertEqual(evidence["before"]["schema_version"], 32)
        self.assertEqual(evidence["after"]["schema_version"], 33)

    def test_v32_migration_requires_the_reviewed_profile_boundary(self) -> None:
        source = self.migration_source("present")
        before_module = str(source["module_sha256"])
        before_digest = str(source["rpc_provider_urls_sha256"])
        result = self.run_driver(
            "--migrate-v32-to-v33",
            "--execute",
            MOCK_SCHEMA="32",
            MOCK_MODULE=before_module,
            MOCK_DIGEST=before_digest,
            MOCK_CAPTURE_BOUNDARY="0x" + "02" * 32,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not match the reviewed frontend profile", result.stderr)
        self.assertFalse(self.install_record.exists())

    def test_v32_migration_verifies_the_persisted_boundary_after_upgrade(self) -> None:
        source = self.migration_source("present")
        before_module = str(source["module_sha256"])
        before_digest = str(source["rpc_provider_urls_sha256"])
        result = self.run_driver(
            "--migrate-v32-to-v33",
            "--execute",
            MOCK_SCHEMA="32",
            MOCK_MODULE=before_module,
            MOCK_DIGEST=before_digest,
            MOCK_BOUNDARY="02" * 32,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("minimum_withdrawal_id does not match", result.stderr)
        self.assertTrue(self.install_record.exists())
        self.assertFalse((self.base / "result.json").exists())

    def test_v32_migration_rejects_metadata_and_module_drift(self) -> None:
        missing_module = str(self.policy()["metadata_missing_module_sha256"])
        after_digest = str(self.policy()["after_rpc_urls_sha256"])
        present = self.run_driver(
            "--migrate-v32-to-v33", "--execute",
            MOCK_SCHEMA="32", MOCK_MODULE=missing_module, MOCK_DIGEST=after_digest,
        )
        self.assertNotEqual(present.returncode, 0)
        self.assertFalse(self.install_record.exists())

        unknown = self.run_driver(
            "--migrate-v32-to-v33", "--execute",
            MOCK_SCHEMA="32", MOCK_MODULE="11" * 32, MOCK_METADATA_MISSING="1", MOCK_DIGEST=after_digest,
        )
        self.assertNotEqual(unknown.returncode, 0)
        self.assertFalse(self.install_record.exists())

    def test_v32_migration_rejects_lookup_failures_before_install(self) -> None:
        missing_module = str(self.policy()["metadata_missing_module_sha256"])
        after_digest = str(self.policy()["after_rpc_urls_sha256"])
        cases = {
            "reader failure": {"MOCK_PUBLIC_METADATA_FAIL": "1"},
            "invalid JSON": {"MOCK_PUBLIC_METADATA_JSON": "not-json"},
            "unknown status": {"MOCK_PUBLIC_METADATA_JSON": json.dumps({"status": "unknown"})},
            "present without value": {
                "MOCK_PUBLIC_METADATA_JSON": json.dumps({"status": "present"})
            },
        }
        for name, changes in cases.items():
            with self.subTest(name=name):
                result = self.run_driver(
                    "--migrate-v32-to-v33", "--execute",
                    MOCK_MODULE=missing_module, MOCK_METADATA_MISSING="1",
                    MOCK_SCHEMA="32", MOCK_DIGEST=after_digest, **changes,
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.install_record.exists())
                self.assertFalse((self.base / "result.json").exists())

    def test_v32_migration_rejects_snapshot_drift_before_install(self) -> None:
        missing_module = str(self.policy()["metadata_missing_module_sha256"])
        after_digest = str(self.policy()["after_rpc_urls_sha256"])
        cases = {
            "canister ID": {"MOCK_CANISTER_ID": "aaaaa-aa"},
            "schema": {"MOCK_SCHEMA": "31"},
            "instance": {"MOCK_INSTANCE": "0x" + "11" * 32},
            "configured chain": {"MOCK_PUBLIC_CHAIN": "1"},
            "EVM RPC canister": {"MOCK_EVM_CANISTER": "aaaaa-aa"},
            "RPC digest": {"MOCK_DIGEST": "22" * 32},
            "status count": {"MOCK_COUNT_DRIFT": "1"},
            "storage integrity": {"MOCK_INTEGRITY_FAIL": "1"},
            "provider chain": {"MOCK_CHAIN_ID": "1"},
        }
        for name, changes in cases.items():
            with self.subTest(name=name):
                result = self.run_driver(
                    "--migrate-v32-to-v33", "--execute",
                    MOCK_MODULE=missing_module, MOCK_METADATA_MISSING="1",
                    **{"MOCK_SCHEMA": "32", "MOCK_DIGEST": after_digest, **changes},
                )
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.install_record.exists())

    def test_candidate_metadata_is_validated_before_live_calls(self) -> None:
        cases = {
            "missing Candid section": {"MOCK_CANDID_SECTION_MISSING": "1"},
            "wrong deployment tag": {"MOCK_CANDID_DEPLOYMENT": "production"},
        }
        for name, changes in cases.items():
            with self.subTest(name=name):
                result = self.run_driver("--execute", **changes)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.install_record.exists())

    def test_unreviewed_ic_host_is_rejected_before_live_calls(self) -> None:
        profile_path = self.repo / "deployments/sepolia-staging/frontend-profile.json"
        profile = json.loads(profile_path.read_text(encoding="utf-8"))
        profile["icHost"] = "https://example.invalid"
        profile_path.write_text(json.dumps(profile), encoding="utf-8")
        self.git("add", "deployments/sepolia-staging/frontend-profile.json")
        self.git("commit", "-qm", "unreviewed IC host")

        result = self.run_driver("--execute")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.install_record.exists())

    def test_missing_profile_boundary_is_rejected_before_live_calls(self) -> None:
        profile_path = self.repo / "deployments/sepolia-staging/frontend-profile.json"
        profile = json.loads(profile_path.read_text(encoding="utf-8"))
        profile["minimumWithdrawalId"] = None
        profile_path.write_text(json.dumps(profile), encoding="utf-8")
        self.git("add", "deployments/sepolia-staging/frontend-profile.json")
        self.git("commit", "-qm", "missing boundary")

        result = self.run_driver("--execute")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("minimumWithdrawalId", result.stderr)
        self.assertFalse(self.install_record.exists())

    def test_preconditions_fail_before_install(self) -> None:
        cases = {
            "wrong chain": {"MOCK_CHAIN_ID": "1"},
            "chain failure": {"MOCK_CHAIN_FAIL": "1"},
            "Candid incompatibility": {"MOCK_DIDC_FAIL": "1"},
            "canister ID drift": {"MOCK_CANISTER_ID": "aaaaa-aa"},
            "schema drift": {"MOCK_SCHEMA": "31"},
            "instance drift": {"MOCK_INSTANCE": "0x" + "11" * 32},
            "configured chain drift": {"MOCK_PUBLIC_CHAIN": "1"},
            "EVM RPC Canister drift": {"MOCK_EVM_CANISTER": "aaaaa-aa"},
            "module drift": {"MOCK_MODULE": "11" * 32},
            "RPC digest drift": {"MOCK_DIGEST": "22" * 32},
            "count drift": {"MOCK_COUNT_DRIFT": "1"},
            "integrity failure": {"MOCK_INTEGRITY_FAIL": "1"},
            "private metadata failure": {"MOCK_PRIVATE_METADATA_FAIL": "1"},
        }
        for name, environment in cases.items():
            with self.subTest(name=name):
                result = self.run_driver("--execute", **environment)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.install_record.exists())

    def test_install_failure_does_not_write_success_evidence(self) -> None:
        result = self.run_driver("--execute", MOCK_INSTALL_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.base / "result.json").exists())

    def test_postcondition_mismatch_does_not_write_success_evidence(self) -> None:
        result = self.run_driver("--execute", MOCK_POST_COUNT_DRIFT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.install_record.exists())
        self.assertTrue((self.state / "applied").exists())
        self.assertFalse((self.base / "result.json").exists())

    def test_policy_rejects_invalid_status_count_types_and_ranges(self) -> None:
        policy_path = self.repo / POLICY_PATH
        cases = (
            ("bool", True),
            ("negative", -1),
            ("u64 overflow", 1 << 64),
            ("u128 overflow", 1 << 128),
        )
        for name, value in cases:
            with self.subTest(name=name):
                policy = json.loads(json.dumps(self.valid_policy))
                field = "reserved_deposit_mint_amount" if name == "u128 overflow" else "deposits"
                policy["status_counts"][field] = value
                policy_path.write_text(json.dumps(policy), encoding="utf-8")
                self.git("add", POLICY_PATH)
                self.git("commit", "-qm", f"invalid {name}")
                result = self.run_driver("--execute")
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.install_record.exists())

        policy = json.loads(json.dumps(self.valid_policy))
        del policy["status_counts"]["deposits"]
        policy_path.write_text(json.dumps(policy), encoding="utf-8")
        self.git("add", POLICY_PATH)
        self.git("commit", "-qm", "missing count")
        self.assertNotEqual(self.run_driver("--execute").returncode, 0)
        self.assertFalse(self.install_record.exists())

    def test_dirty_checkout_and_wasm_mismatch_fail_closed(self) -> None:
        (self.repo / "dirty").write_text("x")
        self.assertNotEqual(self.run_driver().returncode, 0)
        (self.repo / "dirty").unlink()
        self.wasm.write_bytes(b"different")
        self.assertNotEqual(self.run_driver().returncode, 0)


if __name__ == "__main__":
    unittest.main()
