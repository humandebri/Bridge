#!/usr/bin/env python3
"""Regression tests for the fail-closed staging RPC replacement driver."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT_FILES = (
    "scripts/plan007/staging-canister-upgrade.sh",
    "scripts/plan007/staging_canister_upgrade.py",
    "scripts/plan007/candid_values.py",
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
            shutil.copy2(ROOT / relative, destination)
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
            "baseRpcUrl": policy["after_rpc_urls"][0],
            "baseHistoryRpcUrls": policy["after_rpc_urls"][1:],
            "rpcProviderUrlsSha256": "0x" + policy["after_rpc_urls_sha256"],
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

    def write_executable(self, name: str, source: str) -> None:
        path = self.bin / name
        path.write_text(source, encoding="utf-8")
        path.chmod(0o755)

    def write_tools(self) -> None:
        self.write_executable("cast", "#!/bin/sh\n[ \"${MOCK_CHAIN_FAIL:-0}\" = 0 ] || exit 1\necho \"${MOCK_CHAIN_ID:-84532}\"\n")
        self.write_executable("didc", "#!/bin/sh\n[ \"${MOCK_DIDC_FAIL:-0}\" = 0 ] || exit 1\n")
        self.write_executable("icp", r"""#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
state = pathlib.Path(os.environ["MOCK_STATE"])
policy = json.loads(pathlib.Path(os.environ["MOCK_POLICY"]).read_text())
after_module = os.environ["MOCK_AFTER_MODULE"]
applied = (state / "applied").exists() or os.environ.get("MOCK_ALREADY_APPLIED") == "1"
digest = policy["after_rpc_urls_sha256"] if applied else policy["before_rpc_urls_sha256"]
digest = os.environ.get("MOCK_DIGEST", digest)
module = os.environ.get("MOCK_MODULE", after_module if applied else policy["before_module_sha256"])
def blob(value): return ''.join('\\' + value[i:i+2] for i in range(0, len(value), 2))
if args[:2] == ["canister", "install"]:
    (state / "install.json").write_text(json.dumps(args))
    if os.environ.get("MOCK_INSTALL_FAIL") == "1" or os.environ.get("MOCK_POST_COUNT_DRIFT") == "1": raise SystemExit(1)
    (state / "applied").touch(); raise SystemExit(0)
if args[:2] == ["canister", "metadata"]:
    print(json.dumps({"value": "service : {}"})); raise SystemExit(0)
if args[:2] == ["canister", "status"]:
    if "--id-only" in args: print(os.environ.get("MOCK_CANISTER_ID", policy["canister_id"])); raise SystemExit(0)
    print(json.dumps({"module_hash": module})); raise SystemExit(0)
if args[:2] != ["canister", "call"]: raise SystemExit(2)
method = args[3]
if method == "get_public_config":
    schema = os.environ.get("MOCK_SCHEMA", "32")
    instance = os.environ.get("MOCK_INSTANCE", policy["deployment_instance_id"])[2:]
    chain = os.environ.get("MOCK_PUBLIC_CHAIN", str(policy["base_chain_id"]))
    evm = os.environ.get("MOCK_EVM_CANISTER", policy["evm_rpc_canister_id"])
    candid = f'''record {{ schema_version = {schema} : nat16; deployment_instance_id = blob "{blob(instance)}"; base_chain_id = {chain} : nat64; evm_rpc_canister_id = principal "{evm}"; rpc_provider_urls_sha256 = blob "{blob(digest)}" }}'''
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
        self.assertEqual(evidence["result"], "upgraded")
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

        self.install_record.unlink(missing_ok=True)
        (self.state / "applied").unlink(missing_ok=True)
        result = self.run_driver("--execute", MOCK_POST_COUNT_DRIFT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.base / "result.json").exists())
        self.assertFalse((self.state / "applied").exists())

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
