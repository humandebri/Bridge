#!/usr/bin/env python3
"""Regression tests for the fail-closed staging schema upgrade gate."""
from __future__ import annotations

import hashlib, json, os, shutil, subprocess, sys, tempfile, unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from source_resolution import source_path
from staging_canister_upgrade import option_nat64

ROOT = Path(__file__).resolve().parents[2]
SCRIPT_FILES = ("scripts/plan007/staging-canister-upgrade.sh", "scripts/plan007/staging_canister_upgrade.py",
         "scripts/plan007/candid_values.py", "scripts/plan007/read-public-canister-metadata.mjs")
POLICY = "deployments/sepolia-staging/staging-bridge-upgrade-policy.json"
PROFILE = "deployments/sepolia-staging/frontend-profile.json"
COUNTS = {"retained_audit_events": 15, "reconciliation_holds": 0, "retained_deposit_index_entries": 1,
          "pending_ledger_operations": 0, "withdrawals": 1, "deposits": 1,
          "reserved_deposit_mint_operations": 1, "reserved_deposit_mint_amount": 1050000000,
          "pruned_audit_events": 0}


class StagingSchemaUpgradeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory(); self.base = Path(self.temp.name)
        self.repo, self.state, self.bin = self.base / "repo", self.base / "state", self.base / "bin"
        self.repo.mkdir(); self.state.mkdir(); self.bin.mkdir()
        for relative in (*SCRIPT_FILES, POLICY, PROFILE):
            target = self.repo / relative; target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_path(relative) if relative in SCRIPT_FILES else ROOT / relative, target)
        self.did = self.repo / "canister/bridge-canister/bridge.did"; self.did.parent.mkdir(parents=True)
        self.did.write_text("service : { get_runtime_binding : () -> (); get_operational_config : () -> () }")
        source_did = "service : { get_public_config : () -> ()"
        source_did += "; legacy_marker : () -> ()"
        self.source_did = self.base / "source.did"; self.source_did.write_text(source_did + " }")
        current_did = "service : { get_runtime_binding : () -> (); get_operational_config : () -> ()"
        current_did += "; current_marker : () -> ()"
        self.current_did = self.base / "current.did"; self.current_did.write_text(current_did + " }")
        self.wasm = self.base / "candidate.wasm"; self.wasm.write_bytes(b"same schema candidate")
        self.source_module = "11" * 32; self.current_module = "22" * 32; self.target_module = self.sha(self.wasm)
        policy_path = self.repo / POLICY; policy = json.loads(policy_path.read_text())
        policy["source_module_sha256"] = self.source_module; policy["source_candid_sha256"] = self.sha(self.source_did)
        policy["current_schema_source_module_sha256"] = self.current_module
        policy["current_schema_source_candid_sha256"] = self.sha(self.current_did)
        policy_path.write_text(json.dumps(policy))
        self.profile = json.loads((self.repo / PROFILE).read_text())
        self.make_tools()
        self.git("init", "-q"); self.git("config", "user.email", "test@example.invalid"); self.git("config", "user.name", "Test")
        self.git("add", "."); self.git("commit", "-qm", "fixture")
        self.local = self.base / "local.json"
        self.local.write_text(json.dumps({"schema_version": 8,
                                          "source_commit": self.git("rev-parse", "HEAD").stdout.strip(),
                                          "bridge_wasm_sha256": self.target_module, "candid_sha256": self.sha(self.did),
                                          "state_upgrade": {"verified": True},
                                          "tests": {"full_local_ci": "passed"}}))
        self.preflight, self.result = self.base / "preflight.json", self.base / "result.json"

    def tearDown(self) -> None: self.temp.cleanup()
    def sha(self, path: Path) -> str: return hashlib.sha256(path.read_bytes()).hexdigest()
    def git(self, *args: str): return subprocess.run(["git", *args], cwd=self.repo, text=True, capture_output=True, check=True)
    def executable(self, name: str, text: str) -> None:
        path = self.bin / name; path.write_text(text); path.chmod(0o755)

    def make_tools(self) -> None:
        self.executable("cast", r'''#!/usr/bin/env python3
import os,sys
if sys.argv[1:3] != ["chain-id","--rpc-url"]: raise SystemExit(2)
print(os.environ.get("MOCK_CHAIN_ID","84532"))
''')
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
elif os.environ.get("MOCK_CURRENT_SOURCE")=="1": value=pathlib.Path(os.environ["MOCK_CURRENT_DID"]).read_text()
else: value=pathlib.Path(os.environ["MOCK_SOURCE_DID"]).read_text()
print(json.dumps({"status":"present","value":value}))
''')
        self.executable("icp", r'''#!/usr/bin/env python3
import json,os,pathlib,sys
a=sys.argv[1:]; state=pathlib.Path(os.environ["MOCK_STATE"]); profile=json.loads(pathlib.Path(os.environ["MOCK_PROFILE"]).read_text())
applied=state.joinpath("applied").exists() or os.environ.get("MOCK_APPLIED")=="1"
current=os.environ.get("MOCK_CURRENT_SOURCE")=="1"
def esc(v): return ''.join('\\'+v[i:i+2] for i in range(0,len(v),2))
if a[:2]==["canister","install"]:
 state.joinpath("install.json").write_text(json.dumps(a)); state.joinpath("applied").touch(); raise SystemExit(0)
if a[:2]==["canister","metadata"]: print(json.dumps({"value":"test-deployment"})); raise SystemExit(0)
if a[:2]==["canister","status"]:
 if "--id-only" in a: print("rlhjx-iyaaa-aaaaf-qcnyq-cai"); raise SystemExit(0)
 module=os.environ["MOCK_TARGET"] if applied else os.environ.get("MOCK_MODULE",os.environ["MOCK_CURRENT"] if current else os.environ["MOCK_SOURCE"])
 print(json.dumps({"module_hash":module,"settings":{"controllers":["aaaaa-aa"]},"cycles":"1000000000000"})); raise SystemExit(0)
if a[:2] != ["canister","call"]: raise SystemExit(2)
method=a[3]; identity=a[a.index("--identity")+1]
schema=int(os.environ["MOCK_TARGET_SCHEMA"]) if applied or current else 33
marker=1 if applied and os.environ.get("MOCK_BINDING_DRIFT")=="1" else 0
fields=f"""schema_version = {schema} : nat16; deployment_instance_id = blob "{esc(profile['deploymentInstanceId'][2:])}"; minimum_withdrawal_id = blob "{esc(profile['minimumWithdrawalId'][2:])}"; base_chain_id = 84532 : nat64; bridge_contract = blob "{esc(profile['bridgeAddress'][2:])}"; expected_bridge_runtime_sha256 = blob "{esc(profile['bridgeRuntimeHash'][2:])}"; timelock_contract = blob "{esc(profile['timelockAddress'][2:])}"; expected_bridge_signer = blob "{esc(profile['expected_bridge_signer'][2:])}"; ledger_canister_id = principal "{profile['ledgerCanisterId']}"; index_canister_id = principal "{profile['indexCanisterId']}"; evm_rpc_canister_id = principal "{profile['evmRpcCanisterId']}"; rpc_provider_urls_sha256 = blob "{esc(profile['rpcProviderUrlsSha256'][2:])}"; marker = {marker} : nat8"""
if method in ("get_public_config","get_runtime_binding"): candid=f'record {{ {fields}; governance_principal = principal "o3hrk-6xq6w-awts7-vhymn-cs2r2-czkhw-n3zab-6zpvp-5qcz6-hvalv-rae"; cycles_floor = 1000 : nat }}'
elif method=="get_operational_config": candid='variant { Err = variant { Unauthorized } }' if identity=="anonymous" else 'variant { Ok = record { governance_principal = principal "o3hrk-6xq6w-awts7-vhymn-cs2r2-czkhw-n3zab-6zpvp-5qcz6-hvalv-rae"; cycles_floor = 1000 : nat } }'
elif method=="initialize_public_config" and os.environ["MOCK_HARDENING_PROFILE"]=="1": state.joinpath("initialized").touch(); candid='variant { Ok }'
elif method=="get_bridge_status":
 counts=json.loads(os.environ["MOCK_COUNTS"]); counts["deposits"] += int(os.environ.get("MOCK_DRIFT","0"))
 if applied: counts["deposits"] += int(os.environ.get("MOCK_POST_DRIFT","0"))
 candid='record { '+ '; '.join(f'{k} = {v} : '+('nat' if k=='reserved_deposit_mint_amount' else 'nat64') for k,v in counts.items()) +' }'
elif method=="get_audit_events":
 counts=json.loads(os.environ["MOCK_COUNTS"]); marker=1 if applied and os.environ.get("MOCK_AUDIT_DRIFT")=="1" else 0
 requested=int(a[4].split(":",1)[0].lstrip("(")); pruned=counts["pruned_audit_events"]
 first=max(requested,pruned); sequences=list(range(first,min(first+100,pruned+counts["retained_audit_events"])))
 if applied and os.environ.get("MOCK_AUDIT_SEQUENCE_DRIFT")=="1": sequences[-1] += 1
 events='; '.join(f'record {{ sequence = {i} : nat64; marker = {marker} : nat8 }}' for i in sequences)
 observed_pruned=pruned+1 if applied and os.environ.get("MOCK_AUDIT_RETENTION_DRIFT")=="1" else pruned
 through="null" if observed_pruned==0 else f"opt ({observed_pruned-1} : nat64)"
 next_value=sequences[-1]+1 if sequences and sequences[-1]+1 < pruned+counts["retained_audit_events"] else None
 next_sequence="null" if next_value is None else f"opt ({next_value} : nat64)"
 candid=f'variant {{ Ok = record {{ pruned_digest = blob "{esc("00"*32)}"; oldest_available_sequence = {pruned} : nat64; events = vec {{ {events} }}; next_sequence = {next_sequence}; pruned_count = {observed_pruned} : nat64; pruned_through_sequence = {through} }} }}'
elif method=="storage_integrity_check":
 candid='variant { Err = variant { StorageFailure } }' if applied and os.environ.get("MOCK_INTEGRITY_DRIFT")=="1" else 'variant { Ok = "ok" }'
elif method=="get_activation_status": candid='variant { Ok = record { pending_timelock_operation = null } }'
elif method=="get_pending_base_governance_transaction": candid='variant { Ok = vec {} }' if applied and os.environ["MOCK_HARDENING_PROFILE"]=="1" else 'variant { Ok = null }'
else: raise SystemExit(2)
print(json.dumps({"response_candid":candid}))
''')

    def env(self, **changes: str) -> dict[str, str]:
        value = os.environ.copy(); value.update({"PATH": f"{self.bin}{os.pathsep}{value['PATH']}",
            "BRIDGE_STAGING_IDENTITY": "controller", "MOCK_STATE": str(self.state), "MOCK_DID": str(self.did),
            "MOCK_SOURCE_DID": str(self.source_did), "MOCK_CURRENT_DID": str(self.current_did),
            "MOCK_PROFILE": str(self.repo / PROFILE), "MOCK_SOURCE": self.source_module,
            "MOCK_CURRENT": self.current_module, "MOCK_TARGET": self.target_module, "MOCK_COUNTS": json.dumps(COUNTS),
            "MOCK_TARGET_SCHEMA": "34", "MOCK_HARDENING_PROFILE": "1"})
        value.update(changes); return value

    def run_driver(self, execute: bool = False, **changes: str):
        argv = ["bash", str(self.repo / SCRIPT_FILES[0]), "--wasm", str(self.wasm), "--local-evidence", str(self.local),
                "--evidence", str(self.result if execute else self.preflight)]
        if execute: argv += ["--execute", "--preflight-evidence", str(self.preflight)]
        return subprocess.run(argv, cwd=self.repo, env=self.env(**changes), text=True, capture_output=True)

    def test_option_nat64_parser_rejects_wrong_type_and_trailing_tokens(self) -> None:
        self.assertEqual(option_nat64("record { value = opt (7 : nat64); }", "value"), 7)
        self.assertIsNone(option_nat64("record { value = null; }", "value"))
        for invalid in ("opt 7 : nat32", "opt 7garbage", "opt 7 : nat64 garbage"):
            with self.subTest(invalid=invalid), self.assertRaises(SystemExit):
                option_nat64(f"record {{ value = {invalid}; }}", "value")

    def test_preflight_is_read_only_and_records_atomic_counts(self) -> None:
        result = self.run_driver(); self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.state / "install.json").exists())
        evidence = json.loads(self.preflight.read_text())
        self.assertEqual(evidence["result"], "preflight-passed")
        self.assertIn("expected_status_counts = opt record", evidence["upgrade_arguments"])
        migration = "bridge-staging-v33-to-v34"
        self.assertIn(f'migration_id = opt "{migration}"', evidence["upgrade_arguments"])
        self.assertIn("expected_timelock_minimum_delay_seconds = 300", evidence["upgrade_arguments"])
        self.assertIn("expected_minimum_service_fee = 10000", evidence["upgrade_arguments"])
        self.assertIn('confirmation_relayer_principal = opt principal', evidence["upgrade_arguments"])
        self.assertIn("rpc_provider_update = null", evidence["upgrade_arguments"])
        self.assertEqual(evidence["upgrade_arguments"].count("{"), evidence["upgrade_arguments"].count("}"))

    def test_rpc_provider_chain_mismatch_rejects_before_live_state_reads(self) -> None:
        result = self.run_driver(MOCK_CHAIN_ID="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("RPC provider 0 returned an unexpected chain ID", result.stderr)
        self.assertFalse(self.preflight.exists())

    def test_execute_requires_unchanged_preflight_and_upgrades(self) -> None:
        self.assertEqual(self.run_driver().returncode, 0)
        result = self.run_driver(execute=True); self.assertEqual(result.returncode, 0, result.stderr)
        install = json.loads((self.state / "install.json").read_text())
        self.assertEqual(install[install.index("--wasm") + 1], str(self.wasm))
        self.assertTrue((self.state / "initialized").exists())
        self.assertEqual(json.loads(self.result.read_text())["result"], "upgraded")

    def test_current_schema_preflight_uses_non_migrating_guarded_arguments(self) -> None:
        result = self.run_driver(MOCK_CURRENT_SOURCE="1"); self.assertEqual(result.returncode, 0, result.stderr)
        evidence = json.loads(self.preflight.read_text())
        self.assertEqual(evidence["source_kind"], "current-source")
        self.assertEqual(evidence["observed_source_module_sha256"], self.current_module)
        self.assertIn("migration_id = null", evidence["upgrade_arguments"])
        self.assertIn("migration_config = null", evidence["upgrade_arguments"])
        self.assertIn("confirmation_relayer_principal = null", evidence["upgrade_arguments"])
        self.assertIn("expected_status_counts = opt record", evidence["upgrade_arguments"])
        self.assertEqual(evidence["upgrade_arguments"].count("{"), evidence["upgrade_arguments"].count("}"))

    def test_current_schema_preflight_hashes_all_audit_pages_and_pruning_metadata(self) -> None:
        counts = {**COUNTS, "retained_audit_events": 205, "pruned_audit_events": 7}
        result = self.run_driver(MOCK_CURRENT_SOURCE="1", MOCK_COUNTS=json.dumps(counts))
        self.assertEqual(result.returncode, 0, result.stderr)
        audit = json.loads(self.preflight.read_text())["before"]["audit_history"]
        self.assertEqual(audit["event_count"], 205)
        self.assertEqual(audit["page_count"], 3)
        self.assertEqual(audit["first_sequence"], 7)
        self.assertEqual(audit["last_sequence"], 211)
        self.assertEqual(audit["pruned_count"], 7)
        self.assertEqual(audit["pruned_through_sequence"], 6)

    def test_current_schema_execute_upgrades_without_reinitializing_public_config(self) -> None:
        self.assertEqual(self.run_driver(MOCK_CURRENT_SOURCE="1").returncode, 0)
        result = self.run_driver(execute=True, MOCK_CURRENT_SOURCE="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.state / "install.json").exists())
        self.assertFalse((self.state / "initialized").exists())
        self.assertEqual(json.loads(self.result.read_text())["result"], "upgraded")

    def test_current_schema_unknown_module_fails_closed(self) -> None:
        result = self.run_driver(MOCK_CURRENT_SOURCE="1", MOCK_MODULE="33" * 32)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("module and Candid are not a reviewed source", result.stderr)
        self.assertFalse(self.preflight.exists())

    def test_same_candid_with_reviewed_source_module_is_not_misclassified_as_target(self) -> None:
        self.current_did.write_bytes(self.did.read_bytes())
        policy_path = self.repo / POLICY; policy = json.loads(policy_path.read_text())
        policy["current_schema_source_candid_sha256"] = self.sha(self.did)
        policy_path.write_text(json.dumps(policy))
        self.git("add", POLICY); self.git("commit", "-qm", "same candid fixture")
        local = json.loads(self.local.read_text())
        local["source_commit"] = self.git("rev-parse", "HEAD").stdout.strip()
        self.local.write_text(json.dumps(local))
        result = self.run_driver(MOCK_CURRENT_SOURCE="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.preflight.read_text())["source_kind"], "current-source")

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

    def test_audit_content_drift_does_not_write_success_evidence(self) -> None:
        self.assertEqual(self.run_driver(MOCK_CURRENT_SOURCE="1").returncode, 0)
        result = self.run_driver(execute=True, MOCK_CURRENT_SOURCE="1", MOCK_AUDIT_DRIFT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("audit history was not preserved", result.stderr)
        self.assertFalse(self.result.exists())

    def test_audit_sequence_or_retention_drift_does_not_write_success_evidence(self) -> None:
        for change in ({"MOCK_AUDIT_SEQUENCE_DRIFT": "1"}, {"MOCK_AUDIT_RETENTION_DRIFT": "1"}):
            with self.subTest(change=change):
                self.state.joinpath("applied").unlink(missing_ok=True)
                self.assertEqual(self.run_driver(MOCK_CURRENT_SOURCE="1").returncode, 0)
                result = self.run_driver(execute=True, MOCK_CURRENT_SOURCE="1", **change)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.result.exists())

    def test_runtime_binding_drift_does_not_write_success_evidence(self) -> None:
        self.assertEqual(self.run_driver(MOCK_CURRENT_SOURCE="1").returncode, 0)
        result = self.run_driver(execute=True, MOCK_CURRENT_SOURCE="1", MOCK_BINDING_DRIFT="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("runtime binding was not preserved", result.stderr)
        self.assertFalse(self.result.exists())

    def test_integrity_failure_does_not_write_success_evidence(self) -> None:
        self.assertEqual(self.run_driver(MOCK_CURRENT_SOURCE="1").returncode, 0)
        result = self.run_driver(execute=True, MOCK_CURRENT_SOURCE="1", MOCK_INTEGRITY_DRIFT="1")
        self.assertNotEqual(result.returncode, 0)
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

    def test_obsolete_local_evidence_is_rejected(self) -> None:
        value = json.loads(self.local.read_text()); value["schema_version"] = 7
        self.local.write_text(json.dumps(value))
        result = self.run_driver(); self.assertNotEqual(result.returncode, 0)
        self.assertIn("unsupported or incomplete shape", result.stderr)


if __name__ == "__main__": unittest.main()
