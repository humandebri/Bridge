#!/usr/bin/env python3
"""Gate and execute the reviewed staging Bridge v33-to-v36 upgrade."""
from __future__ import annotations

import argparse, hashlib, json, os, re, shutil, subprocess, tempfile
from pathlib import Path
from typing import Any
from candid_values import blob, integrity_ok, nat, principal

ROOT = Path(__file__).resolve().parents[2]
POLICY = ROOT / "deployments/sepolia-staging/same-schema-upgrade-policy.json"
PROFILE = ROOT / "deployments/sepolia-staging/frontend-profile.json"
DID = ROOT / "canister/bridge-canister/bridge.did"
METADATA_READER = ROOT / "scripts/plan007/read-public-canister-metadata.mjs"
IC_HOST = "https://icp-api.io"
LOCAL_E2E_SCHEMA_VERSION = 8
COUNTS = ("retained_audit_events", "reconciliation_holds", "retained_deposit_index_entries",
          "pending_ledger_operations", "withdrawals", "deposits",
          "reserved_deposit_mint_operations", "reserved_deposit_mint_amount", "pruned_audit_events")
PRESERVED = ("canister_id", "deployment_instance_id", "minimum_withdrawal_id",
             "base_chain_id", "bridge_contract", "expected_bridge_runtime_sha256", "timelock_contract",
             "expected_bridge_signer", "ledger_canister_id", "index_canister_id", "evm_rpc_canister_id",
             "rpc_provider_urls_sha256", "governance_principal", "status_counts", "storage_integrity",
             "pending_timelock_operations", "pending_governance_transactions", "controllers", "cycles_floor")
MIGRATION_ID = "bridge-staging-v33-to-v36"
SHA = re.compile(r"[0-9a-f]{64}")
PRINCIPAL = re.compile(r"[a-z0-9-]+")


def fail(message: str) -> None: raise SystemExit(message)


def run(argv: list[str], *, capture: bool = True) -> str:
    result = subprocess.run(argv, cwd=ROOT, text=True, capture_output=capture, check=False)
    if result.returncode:
        fail(f"command failed ({' '.join(argv[:3])}): {result.stderr.strip() if capture else result.returncode}")
    return result.stdout if capture else ""


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""): value.update(chunk)
    return value.hexdigest()


def load(path: Path, context: str) -> dict[str, Any]:
    try: value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error: fail(f"invalid {context}: {error}")
    if not isinstance(value, dict): fail(f"{context} must be a JSON object")
    return value


def validate_policy(value: dict[str, Any]) -> None:
    fields = {"schema_version", "kind", "environment", "canister_name", "canister_id",
              "stable_schema_version", "source_schema_version", "source_wire_version",
              "deployment_instance_id", "base_chain_id", "evm_rpc_canister_id",
              "governance_principal", "source_module_sha256", "source_candid_sha256", "source_api", "target_api"}
    if set(value) != fields or value["schema_version"] != 1 or value["kind"] != "staging-bridge-v33-to-v36-upgrade":
        fail("v33-to-v36 upgrade policy has an unsupported shape")
    if value["stable_schema_version"] != 36 or value["source_schema_version"] != 33 \
            or value["source_wire_version"] != 28 or value["source_api"] != "get_public_config" \
            or value["target_api"] != "get_runtime_binding":
        fail("policy does not bind the reviewed stable schema v33-to-v36 transition")
    if any(not isinstance(value[f], str) or not SHA.fullmatch(value[f])
           for f in ("source_module_sha256", "source_candid_sha256")):
        fail("policy source hashes must be lowercase SHA-256 digests")
    if not re.fullmatch(r"0x[0-9a-f]{64}", str(value["deployment_instance_id"])):
        fail("policy deployment instance ID is invalid")
    if any(not isinstance(value[f], str) or not PRINCIPAL.fullmatch(value[f])
           for f in ("canister_id", "evm_rpc_canister_id", "governance_principal")):
        fail("policy contains an invalid principal")


def base(policy: dict[str, Any], identity: str) -> list[str]:
    return ["-e", policy["environment"], "--identity", identity, "--project-root-override", str(ROOT)]


def call(policy: dict[str, Any], identity: str, did: Path, method: str) -> str:
    output = run(["icp", "canister", "call", policy["canister_name"], method, "()", "--query",
                  "--candid", str(did), "--json", *base(policy, identity)])
    try: value = json.loads(output).get("response_candid")
    except json.JSONDecodeError: fail(f"{method} returned invalid JSON")
    if not isinstance(value, str): fail(f"{method} did not return response_candid")
    return value


def values(value: Any, key: str) -> list[Any]:
    found: list[Any] = []
    if isinstance(value, dict):
        for name, child in value.items():
            if name == key: found.append(child)
            found.extend(values(child, key))
    elif isinstance(value, list):
        for child in value: found.extend(values(child, key))
    return found


def status(policy: dict[str, Any], identity: str) -> dict[str, Any]:
    try: value = json.loads(run(["icp", "canister", "status", policy["canister_name"], "--json", *base(policy, identity)]))
    except json.JSONDecodeError: fail("canister status returned invalid JSON")
    modules, controller_sets = values(value, "module_hash"), values(value, "controllers")
    cycle_values = values(value, "cycles") or values(value, "cycles_balance")
    if len(modules) != 1 or not SHA.fullmatch(str(modules[0]).lower().removeprefix("0x")):
        fail("canister status must expose one valid module hash")
    if len(controller_sets) != 1 or not isinstance(controller_sets[0], list):
        fail("canister status must expose one controller set")
    controllers = sorted(str(item) for item in controller_sets[0])
    if not controllers or any(not PRINCIPAL.fullmatch(item) for item in controllers): fail("invalid controller set")
    if len(cycle_values) != 1: fail("canister status must expose one cycles balance")
    text = str(cycle_values[0]).strip().strip('"').replace("_", "")
    cycles = int(text, 16) if text.startswith("0x") else int(re.sub(r"[^0-9]", "", text) or "0")
    if cycles <= 0: fail("canister cycles balance must be positive")
    return {"module_sha256": str(modules[0]).lower().removeprefix("0x"),
            "controllers": controllers, "cycles_balance": cycles}


def pending_count(candid: str) -> int:
    found = re.findall(r"\bpending_timelock_operation\s*=\s*(null|opt\b)", candid)
    if len(found) != 1: fail("activation status must expose one pending Timelock operation")
    return 0 if found[0] == "null" else 1


def pending_governance_count(candid: str) -> int:
    if re.search(r"\bOk\s*=\s*null\b", candid): return 0
    if re.search(r"\bOk\s*=\s*opt\b", candid): return 1
    fail("governance status must expose one pending transaction")


def snapshot(policy: dict[str, Any], identity: str, did: Path, api: str, candid_hash: str) -> dict[str, Any]:
    binding = call(policy, identity, did, api)
    bridge_status = call(policy, identity, did, "get_bridge_status")
    integrity = call(policy, identity, did, "storage_integrity_check")
    activation = call(policy, identity, did, "get_activation_status")
    governance = call(policy, identity, did, "get_pending_base_governance_transaction")
    operational = binding if api == "get_public_config" else call(policy, identity, did, "get_operational_config")
    if api == "get_runtime_binding" and not re.search(r"\bOk\s*=", operational):
        fail("authorized get_operational_config did not succeed")
    canister = status(policy, identity)
    canister_id = run(["icp", "canister", "status", policy["canister_name"], "--id-only", *base(policy, identity)]).strip()
    try:
        result = {
            "api": api, "candid_sha256": candid_hash, "canister_id": canister_id, **canister,
            "schema_version": nat(binding, "schema_version"),
            "deployment_instance_id": "0x" + blob(binding, "deployment_instance_id", length=32).hex(),
            "minimum_withdrawal_id": "0x" + blob(binding, "minimum_withdrawal_id", length=32).hex(),
            "base_chain_id": nat(binding, "base_chain_id"),
            "bridge_contract": "0x" + blob(binding, "bridge_contract", length=20).hex(),
            "expected_bridge_runtime_sha256": "0x" + blob(binding, "expected_bridge_runtime_sha256", length=32).hex(),
            "timelock_contract": "0x" + blob(binding, "timelock_contract", length=20).hex(),
            "expected_bridge_signer": "0x" + blob(binding, "expected_bridge_signer", length=20).hex(),
            "ledger_canister_id": principal(binding, "ledger_canister_id"),
            "index_canister_id": principal(binding, "index_canister_id"),
            "evm_rpc_canister_id": principal(binding, "evm_rpc_canister_id"),
            "rpc_provider_urls_sha256": "0x" + blob(binding, "rpc_provider_urls_sha256", length=32).hex(),
            "governance_principal": principal(operational, "governance_principal"),
            "cycles_floor": nat(operational, "cycles_floor"),
            "status_counts": {field: nat(bridge_status, field) for field in COUNTS},
            "storage_integrity": "ok" if integrity_ok(integrity) else "failed",
            "pending_timelock_operations": pending_count(activation),
            "pending_governance_transactions": pending_governance_count(governance),
        }
    except ValueError as error: fail(f"invalid {api} snapshot: {error}")
    return result


def verify_binding(observed: dict[str, Any], policy: dict[str, Any], profile: dict[str, Any], schema: int) -> None:
    expected = {"canister_id": policy["canister_id"], "schema_version": schema,
                "deployment_instance_id": policy["deployment_instance_id"],
                "minimum_withdrawal_id": profile["minimumWithdrawalId"], "base_chain_id": policy["base_chain_id"],
                "bridge_contract": profile["bridgeAddress"], "expected_bridge_runtime_sha256": profile["bridgeRuntimeHash"],
                "timelock_contract": profile["timelockAddress"], "expected_bridge_signer": profile["expected_bridge_signer"],
                "ledger_canister_id": profile["ledgerCanisterId"], "index_canister_id": profile["indexCanisterId"],
                "evm_rpc_canister_id": policy["evm_rpc_canister_id"],
                "rpc_provider_urls_sha256": profile["rpcProviderUrlsSha256"],
                "governance_principal": policy["governance_principal"], "storage_integrity": "ok"}
    for field, value in expected.items():
        if observed.get(field) != value: fail(f"live snapshot {field} does not match the reviewed staging binding")
    if observed["cycles_balance"] < observed["cycles_floor"]:
        fail("live cycles balance is below the operational cycles floor")
    if observed["pending_timelock_operations"] != 0 or observed["pending_governance_transactions"] != 0:
        fail("live staging governance queues must be empty")


def verify_provider_chains(profile: dict[str, Any], expected_chain_id: int) -> None:
    primary = profile.get("baseRpcUrl")
    history = profile.get("baseHistoryRpcUrls")
    if not isinstance(primary, str) or not isinstance(history, list) \
            or len(history) != 2 or not all(isinstance(url, str) for url in history):
        fail("frontend profile must define one primary and two history RPC providers")
    urls = [primary, *history]
    if len(set(urls)) != 3 or any(not url.startswith("https://") for url in urls):
        fail("frontend profile RPC providers must be distinct HTTPS URLs")
    observed_digest = "0x" + hashlib.sha256(
        json.dumps(urls, separators=(",", ":")).encode()
    ).hexdigest()
    if observed_digest != profile.get("rpcProviderUrlsSha256"):
        fail("frontend profile RPC provider digest does not match its URLs")
    for index, url in enumerate(urls):
        observed = run(["cast", "chain-id", "--rpc-url", url]).strip()
        if observed != str(expected_chain_id):
            fail(f"staging RPC provider {index} returned an unexpected chain ID")


def classify(candid: str) -> str:
    old = bool(re.search(r"\bget_public_config\s*:", candid))
    new = bool(re.search(r"\bget_runtime_binding\s*:", candid)) and bool(re.search(r"\bget_operational_config\s*:", candid))
    if old and not new: return "source"
    if new and not old: return "target"
    fail("live Candid is neither the reviewed source nor target API shape")


def live_candid(policy: dict[str, Any]) -> str:
    output = run(["node", str(METADATA_READER), IC_HOST, policy["canister_id"], "candid:service"])
    try: value = json.loads(output)
    except json.JSONDecodeError: fail("certified live Candid lookup returned invalid JSON")
    if not isinstance(value, dict) or set(value) != {"status", "value"} or value["status"] != "present" or not isinstance(value["value"], str):
        fail("live canister must expose certified candid:service metadata")
    return value["value"]


def candidate(wasm: Path) -> None:
    sections = run(["ic-wasm", str(wasm), "metadata"]).splitlines()
    if sections.count("icp:public candid:service") != 1 or sections.count("icp:private kinic:deployment") != 1:
        fail("explicit Wasm metadata sections are invalid")
    candid = run(["ic-wasm", str(wasm), "metadata", "candid:service"]).removesuffix("\n")
    if candid.encode() != DID.read_bytes() or classify(candid) != "target":
        fail("explicit Wasm Candid metadata does not match the target interface")
    if run(["ic-wasm", str(wasm), "metadata", "kinic:deployment"]).strip() != "test-deployment":
        fail("explicit Wasm deployment metadata is invalid")


def private_metadata(policy: dict[str, Any], identity: str) -> None:
    output = run(["icp", "canister", "metadata", policy["canister_name"], "kinic:deployment", "--json", *base(policy, identity)])
    try: value = json.loads(output)
    except json.JSONDecodeError: fail("live deployment metadata is invalid JSON")
    if value != {"value": "test-deployment"}: fail("live deployment metadata is invalid")


def verify_auth(policy: dict[str, Any], identity: str) -> None:
    denied = call(policy, "anonymous", DID, "get_operational_config")
    if not re.search(r"\bErr\s*=\s*variant\s*\{\s*Unauthorized\b", denied):
        fail("anonymous get_operational_config did not return Unauthorized")
    allowed = call(policy, identity, DID, "get_operational_config")
    try: governance = principal(allowed, "governance_principal")
    except ValueError as error: fail(f"authorized operational config is invalid: {error}")
    if not re.search(r"\bOk\s*=", allowed) or governance != policy["governance_principal"]:
        fail("controller or governance get_operational_config did not return the reviewed config")


def upgrade_args(counts: dict[str, Any], policy: dict[str, Any]) -> str:
    fields = "; ".join(f"{field} = {counts[field]} : {'nat' if field == 'reserved_deposit_mint_amount' else 'nat64'}" for field in COUNTS)
    return (f'(record {{ migration_id = opt "{MIGRATION_ID}"; status_counts_guard_version = 1 : nat8; expected_status_counts = opt record {{ '
            f"{fields}" + " }; rpc_provider_update = null; minimum_withdrawal_id = null; "
            f'confirmation_relayer_principal = opt principal "{policy["governance_principal"]}" }})')


def write(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def verify_preflight_unchanged(reviewed: dict[str, Any], current: dict[str, Any]) -> None:
    reviewed_copy = json.loads(json.dumps(reviewed))
    current_copy = json.loads(json.dumps(current))
    try:
        reviewed_cycles = reviewed_copy["before"].pop("cycles_balance")
        current_cycles = current_copy["before"].pop("cycles_balance")
    except (KeyError, TypeError):
        fail("preflight evidence has an invalid cycles snapshot")
    if reviewed_copy != current_copy:
        fail("live state or inputs drifted from preflight evidence")
    if not isinstance(reviewed_cycles, int) or isinstance(reviewed_cycles, bool) \
            or not isinstance(current_cycles, int) or isinstance(current_cycles, bool):
        fail("preflight evidence has an invalid cycles balance")
    allowance = max(10_000_000_000, reviewed_cycles // 100)
    if current_cycles > reviewed_cycles or reviewed_cycles - current_cycles > allowance:
        fail("live cycles balance drifted materially from preflight evidence")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--wasm", type=Path, required=True)
    parser.add_argument("--local-evidence", type=Path, required=True)
    parser.add_argument("--preflight-evidence", type=Path)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    if not args.wasm.is_absolute() or not args.wasm.is_file(): fail("--wasm must name an existing absolute file")
    if not args.local_evidence.is_absolute() or not args.local_evidence.is_file(): fail("--local-evidence must name an existing absolute file")
    if not args.evidence.is_absolute(): fail("--evidence must be an absolute path")
    if args.execute != (args.preflight_evidence is not None):
        fail("--execute requires --preflight-evidence, which is invalid during preflight")
    if args.preflight_evidence is not None and not args.preflight_evidence.is_absolute(): fail("--preflight-evidence must be absolute")
    identity = os.environ.get("BRIDGE_STAGING_IDENTITY")
    if not identity: fail("BRIDGE_STAGING_IDENTITY is required")
    for tool in ("cast", "git", "ic-wasm", "icp", "node"):
        if shutil.which(tool) is None: fail(f"{tool} is required")
    if run(["git", "status", "--porcelain", "--untracked-files=all"]).strip(): fail("upgrade requires a clean checkout")
    head = run(["git", "rev-parse", "HEAD"]).strip()
    policy, profile = load(POLICY, "v33-to-v36 policy"), load(PROFILE, "frontend profile")
    validate_policy(policy)
    if profile.get("icHost") != IC_HOST: fail("frontend profile IC host is invalid")
    for field, expected in {"environment": policy["environment"], "bridgeCanisterId": policy["canister_id"],
                            "deploymentInstanceId": policy["deployment_instance_id"], "chainId": policy["base_chain_id"],
                            "evmRpcCanisterId": policy["evm_rpc_canister_id"]}.items():
        if profile.get(field) != expected: fail(f"frontend profile {field} does not match policy")
    verify_provider_chains(profile, policy["base_chain_id"])
    local = load(args.local_evidence, "local E2E evidence")
    if local.get("schema_version") != LOCAL_E2E_SCHEMA_VERSION \
            or local.get("state_upgrade", {}).get("verified") is not True \
            or set(local.get("tests", {}).values()) != {"passed"}:
        fail("local E2E evidence has an unsupported or incomplete shape")
    if local.get("source_commit") != head: fail("local E2E evidence source commit must equal clean HEAD")
    if local.get("bridge_wasm_sha256") != digest(args.wasm) or local.get("candid_sha256") != digest(DID):
        fail("local E2E evidence does not bind the explicit Wasm and Candid")
    candidate(args.wasm); private_metadata(policy, identity)
    target_module, target_candid = digest(args.wasm), digest(DID)
    live = live_candid(policy); live_hash = hashlib.sha256(live.encode()).hexdigest(); kind = classify(live)
    if kind == "source" and live_hash != policy["source_candid_sha256"]: fail("live source Candid hash is unknown")
    if kind == "target" and live_hash != target_candid: fail("live target Candid hash is unknown")
    api = policy["source_api"] if kind == "source" else policy["target_api"]
    with tempfile.NamedTemporaryFile("w", suffix=".did", encoding="utf-8") as live_did:
        live_did.write(live); live_did.flush()
        before = snapshot(policy, identity, Path(live_did.name), api, live_hash)
    verify_binding(before, policy, profile,
                   policy["source_schema_version"] if kind == "source" else policy["stable_schema_version"])
    if kind == "source" and before["module_sha256"] != policy["source_module_sha256"]: fail("live source module hash is unknown")
    if kind == "target" and before["module_sha256"] != target_module: fail("live target module hash is unknown")
    if kind == "target": verify_auth(policy, identity)
    arguments = upgrade_args(before["status_counts"], policy)
    preflight = {"schema_version": 1, "kind": "staging-bridge-v33-to-v36-upgrade-preflight",
                 "result": "already-applied-preflight" if kind == "target" else "preflight-passed",
                 "source_commit": head, "local_e2e_sha256": digest(args.local_evidence),
                 "policy_sha256": digest(POLICY), "profile_sha256": digest(PROFILE),
                 "source_module_sha256": policy["source_module_sha256"],
                 "source_candid_sha256": policy["source_candid_sha256"],
                 "target_module_sha256": target_module, "target_candid_sha256": target_candid,
                 "upgrade_arguments": arguments, "before": before}
    if not args.execute:
        write(args.evidence, preflight); print(json.dumps({"result": preflight["result"], "evidence": str(args.evidence)})); return
    verify_preflight_unchanged(load(args.preflight_evidence, "preflight evidence"), preflight)
    if kind == "source":
        run(["icp", "canister", "install", policy["canister_name"], "--mode", "upgrade", "--wasm", str(args.wasm),
             "--args", arguments, "--yes", *base(policy, identity)], capture=False); result = "upgraded"
    else: result = "already-applied"
    after_live = live_candid(policy)
    if after_live.encode() != DID.read_bytes(): fail("post-upgrade certified Candid does not match target")
    after = snapshot(policy, identity, DID, policy["target_api"], hashlib.sha256(after_live.encode()).hexdigest())
    if after["module_sha256"] != target_module: fail("post-upgrade module hash does not match target")
    verify_binding(after, policy, profile, policy["stable_schema_version"])
    for field in PRESERVED:
        if after[field] != before[field]: fail(f"post-upgrade {field} was not preserved")
    if after["cycles_balance"] < after["cycles_floor"] or after["cycles_balance"] > before["cycles_balance"]:
        fail("post-upgrade cycles balance is invalid")
    verify_auth(policy, identity)
    write(args.evidence, {**preflight, "kind": "staging-bridge-v33-to-v36-upgrade-result", "result": result,
                          "preflight_evidence_sha256": digest(args.preflight_evidence), "after": after})
    print(f"staging Bridge v33-to-v36 upgrade verified: {result}")


if __name__ == "__main__": main()
