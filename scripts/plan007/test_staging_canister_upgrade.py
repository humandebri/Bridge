#!/usr/bin/env python3
"""Regression tests for the fail-closed staging same-schema upgrade gate."""
from __future__ import annotations

import hashlib, json, os, shutil, subprocess, sys, tempfile, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from source_resolution import source_path

ROOT = Path(__file__).resolve().parents[2]
FILES = ("scripts/plan007/staging-canister-upgrade.sh", "scripts/plan007/staging_canister_upgrade.py",
         "scripts/plan007/candid_values.py", "scripts/plan007/read-public-canister-metadata.mjs")
POLICY = "deployments/sepolia-staging/same-schema-upgrade-policy.json"
PROFILE = "deployments/sepolia-staging/frontend-profile.json"
COUNTS = {"retained_audit_events": 15, "reconciliation_holds": 0, "retained_deposit_index_entries": 1,
          "pending_ledger_operations": 0, "withdrawals": 1, "deposits": 1,
          "reserved_deposit_mint_operations": 1, "reserved_deposit_mint_amount": 1050000000,
          "pruned_audit_events": 0}


class SameSchemaUpgradeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(); self.base = Path(self.temp.name)
        self.repo, self.state, self.bin = self.base / "repo", self.base / "state", self.base / "bin"
        self.repo.mkdir(); self.state.mkdir(); self.bin.mkdir()
        for relative in (*FILES, POLICY, PROFILE):
            target = self.repo / relative; target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_path(relative) if relative in FILES else ROOT / relative, target)
        self.did = self.repo / "canister/bridge-canister/bridge.did"; self.did.parent.mkdir(parents=True)
        self.did.write_text("service : { get_runtime_binding : () -> (); get_operational_config : () -> () }")
        self.source_did = self.base / "source.did"; self.source_did.write_text("service : { get_public_config : () -> () }")
        self.wasm = self.base / "candidate.wasm"; self.wasm.write_bytes(b"same schema candidate")
        self.source_module = "11" * 32; self.target_module = self.sha(self.wasm)
        policy_path = self.repo / POLICY; policy = json.loads(policy_path.read_text())
        policy["source_module_sha256"] = self.source_module; policy["source_candid_sha256"] = self.sha(self.source_did)
        policy_path.write_text(json.dumps(policy))
        self.profile = json.loads((self.repo / PROFILE).read_text())
        self.make_tools()
        self.git("init", "-q"); self.git("config", "user.email", "test@example.invalid"); self.git("config", "user.name", "Test")
        self.git("add", "."); self.git("commit", "-qm", "fixture")
        self.local = self.base / "local.json"
        self.local.write_text(json.dumps({"source_commit": self.git("rev-parse", "HEAD").stdout.strip(),
                                          "bridge_wasm_sha256": self.target_module, "candid_sha256": self.sha(self.did)}))
        self.preflight, self.result = self.base / "preflight.json", self.base / "result.json"

    def tearDown(self) -> None: self.temp.cleanup()
    def sha(self, path: Path) -> str: return hashlib.sha256(path.read_bytes()).hexdigest()
    def git(self, *args: str): return subprocess.run(["git", *args], cwd=self.repo, text=True, capture_output=True, check=True)
    def executable(self, name: str, text: str) -> None:
        path = self.bin / name; path.write_text(text); path.chmod(0o755)

    def make_tools(self) -> None:
        self.executable("ic-wasm", r'''#!/usr/bin/env python3
import os,pathlib,sys
a=sys.argv[1:]
if a[-1]=="metadata": print("icp:public candid:service\nicp:private kinic:deployment")
elif a[-2:]==["metadata","candid:service"]: print(pathlib.Path(os.environ["MOCK_DID"]).read_text(),end="")
elif a[-2:]==["metadata","kinic:deployment"]: print("test-deployment")
else: raise SystemExit(2)
''')
        self.executable("node", r'''#!/usr/bin/env python3
import json,os,pathlib,sys
state=pathlib.Path(os.environ["MOCK_STATE"])
if os.environ.get("MOCK_UNKNOWN_CANDID")=="1": value="service : {}\n"
elif state.joinpath("applied").exists() or os.environ.get("MOCK_APPLIED")=="1": value=pathlib.Path(os.environ["MOCK_DID"]).read_text()
else: value=pathlib.Path(os.environ["MOCK_SOURCE_DID"]).read_text()
print(json.dumps({"status":"present","value":value}))
''')
        self.executable("icp", r'''#!/usr/bin/env python3
import json,os,pathlib,sys
a=sys.argv[1:]; state=pathlib.Path(os.environ["MOCK_STATE"]); profile=json.loads(pathlib.Path(os.environ["MOCK_PROFILE"]).read_text())
applied=state.joinpath("applied").exists() or os.environ.get("MOCK_APPLIED")=="1"
def esc(v): return ''.join('\\'+v[i:i+2] for i in range(0,len(v),2))
if a[:2]==["canister","install"]:
 state.joinpath("install.json").write_text(json.dumps(a)); state.joinpath("applied").touch(); raise SystemExit(0)
if a[:2]==["canister","metadata"]: print(json.dumps({"value":"test-deployment"})); raise SystemExit(0)
if a[:2]==["canister","status"]:
 if "--id-only" in a: print("rlhjx-iyaaa-aaaaf-qcnyq-cai"); raise SystemExit(0)
 module=os.environ["MOCK_TARGET"] if applied else os.environ.get("MOCK_MODULE",os.environ["MOCK_SOURCE"])
 print(json.dumps({"module_hash":module,"settings":{"controllers":["aaaaa-aa"]},"cycles":"1000000000000"})); raise SystemExit(0)
if a[:2] != ["canister","call"]: raise SystemExit(2)
method=a[3]; identity=a[a.index("--identity")+1]
fields=f"""schema_version = 35 : nat16; deployment_instance_id = blob "{esc(profile['deploymentInstanceId'][2:])}"; minimum_withdrawal_id = blob "{esc(profile['minimumWithdrawalId'][2:])}"; base_chain_id = 84532 : nat64; bridge_contract = blob "{esc(profile['bridgeAddress'][2:])}"; expected_bridge_runtime_sha256 = blob "{esc(profile['bridgeRuntimeHash'][2:])}"; timelock_contract = blob "{esc(profile['timelockAddress'][2:])}"; expected_bridge_signer = blob "{esc(profile['expected_bridge_signer'][2:])}"; ledger_canister_id = principal "{profile['ledgerCanisterId']}"; index_canister_id = principal "{profile['indexCanisterId']}"; evm_rpc_canister_id = principal "{profile['evmRpcCanisterId']}"; rpc_provider_urls_sha256 = blob "{esc(profile['rpcProviderUrlsSha256'][2:])}"; marker = 0 : nat8"""
if method in ("get_public_config","get_runtime_binding"): candid=f'record {{ {fields}; governance_principal = principal "o3hrk-6xq6w-awts7-vhymn-cs2r2-czkhw-n3zab-6zpvp-5qcz6-hvalv-rae"; cycles_floor = 1000 : nat }}'
elif method=="get_operational_config": candid='variant { Err = variant { Unauthorized } }' if identity=="anonymous" else 'variant { Ok = record { governance_principal = principal "o3hrk-6xq6w-awts7-vhymn-cs2r2-czkhw-n3zab-6zpvp-5qcz6-hvalv-rae"; cycles_floor = 1000 : nat } }'
elif method=="get_bridge_status":
 counts=json.loads(os.environ["MOCK_COUNTS"]); counts["deposits"] += int(os.environ.get("MOCK_DRIFT","0"))
 if applied: counts["deposits"] += int(os.environ.get("MOCK_POST_DRIFT","0"))
 candid='record { '+ '; '.join(f'{k} = {v} : '+('nat' if k=='reserved_deposit_mint_amount' else 'nat64') for k,v in counts.items()) +' }'
elif method=="storage_integrity_check": candid='variant { Ok = "ok" }'
elif method=="get_activation_status": candid='variant { Ok = record { pending_timelock_operation = null } }'
else: raise SystemExit(2)
print(json.dumps({"response_candid":candid}))
''')

    def env(self, **changes: str) -> dict[str, str]:
        value = os.environ.copy(); value.update({"PATH": f"{self.bin}{os.pathsep}{value['PATH']}",
            "BRIDGE_STAGING_IDENTITY": "controller", "MOCK_STATE": str(self.state), "MOCK_DID": str(self.did),
            "MOCK_SOURCE_DID": str(self.source_did), "MOCK_PROFILE": str(self.repo / PROFILE),
            "MOCK_SOURCE": self.source_module, "MOCK_TARGET": self.target_module, "MOCK_COUNTS": json.dumps(COUNTS)})
        value.update(changes); return value

    def run_driver(self, execute: bool = False, **changes: str):
        argv = ["bash", str(self.repo / FILES[0]), "--wasm", str(self.wasm), "--local-evidence", str(self.local),
                "--evidence", str(self.result if execute else self.preflight)]
        if execute: argv += ["--execute", "--preflight-evidence", str(self.preflight)]
        return subprocess.run(argv, cwd=self.repo, env=self.env(**changes), text=True, capture_output=True)

    def test_preflight_is_read_only_and_records_atomic_counts(self) -> None:
        result = self.run_driver(); self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.state / "install.json").exists())
        evidence = json.loads(self.preflight.read_text())
        self.assertEqual(evidence["result"], "preflight-passed")
        self.assertIn("expected_status_counts = opt record", evidence["upgrade_arguments"])
        self.assertIn("rpc_provider_update = null", evidence["upgrade_arguments"])

    def test_execute_requires_unchanged_preflight_and_upgrades(self) -> None:
        self.assertEqual(self.run_driver().returncode, 0)
        result = self.run_driver(execute=True); self.assertEqual(result.returncode, 0, result.stderr)
        install = json.loads((self.state / "install.json").read_text())
        self.assertEqual(install[install.index("--wasm") + 1], str(self.wasm))
        self.assertEqual(json.loads(self.result.read_text())["result"], "upgraded")

    def test_state_drift_rejects_before_install(self) -> None:
        self.assertEqual(self.run_driver().returncode, 0)
        result = self.run_driver(execute=True, MOCK_DRIFT="1")
        self.assertNotEqual(result.returncode, 0); self.assertFalse((self.state / "install.json").exists())

    def test_tampered_preflight_is_rejected(self) -> None:
        self.assertEqual(self.run_driver().returncode, 0)
        value = json.loads(self.preflight.read_text()); value["before"]["status_counts"]["deposits"] += 1
        self.preflight.write_text(json.dumps(value))
        result = self.run_driver(execute=True); self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.state / "install.json").exists())

    def test_postcondition_drift_does_not_write_success_evidence(self) -> None:
        self.assertEqual(self.run_driver().returncode, 0)
        result = self.run_driver(execute=True, MOCK_POST_DRIFT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.state / "install.json").exists())
        self.assertFalse(self.result.exists())

    def test_already_applied_skips_install(self) -> None:
        self.assertEqual(self.run_driver(MOCK_APPLIED="1").returncode, 0)
        result = self.run_driver(execute=True, MOCK_APPLIED="1"); self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.state / "install.json").exists())
        self.assertEqual(json.loads(self.result.read_text())["result"], "already-applied")

    def test_unknown_candid_and_dirty_checkout_fail_closed(self) -> None:
        unknown = self.run_driver(MOCK_UNKNOWN_CANDID="1"); self.assertNotEqual(unknown.returncode, 0)
        (self.repo / "dirty").write_text("x")
        dirty = self.run_driver(); self.assertNotEqual(dirty.returncode, 0); self.assertIn("clean checkout", dirty.stderr)


if __name__ == "__main__": unittest.main()
