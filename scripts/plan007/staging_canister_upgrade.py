#!/usr/bin/env python3
"""Preflight and execute the one reviewed Base Sepolia RPC provider replacement."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any

from candid_values import blob, integrity_ok, nat, principal

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_POLICY = ROOT / "deployments/sepolia-staging/rpc-provider-replacement-policy.json"
DEFAULT_PROFILE = ROOT / "deployments/sepolia-staging/frontend-profile.json"
DEFAULT_LOCAL_EVIDENCE = ROOT / "deployments/sepolia-staging/evidence/local-e2e.json"
DEFAULT_DID = ROOT / "canister/bridge-canister/bridge.did"
PUBLIC_METADATA_READER = ROOT / "scripts/plan007/read-public-canister-metadata.mjs"
IC_MAINNET_HOST = "https://icp-api.io"
COUNT_FIELDS = (
    "retained_audit_events",
    "reconciliation_holds",
    "retained_deposit_index_entries",
    "pending_ledger_operations",
    "withdrawals",
    "deposits",
    "reserved_deposit_mint_operations",
    "reserved_deposit_mint_amount",
    "pruned_audit_events",
)
U64_MAX = (1 << 64) - 1
U128_MAX = (1 << 128) - 1


def fail(message: str) -> None:
    raise SystemExit(message)


def run(command: list[str], *, capture: bool = True) -> str:
    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=capture, check=False)
    if result.returncode:
        detail = result.stderr.strip() if capture else ""
        fail(f"command failed ({' '.join(command[:3])}): {detail or result.returncode}")
    return result.stdout if capture else ""


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_object(path: Path, context: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"invalid {context}: {error}")
    if not isinstance(value, dict):
        fail(f"{context} must be a JSON object")
    return value


def rpc_digest(urls: list[str]) -> str:
    encoded = json.dumps(urls, ensure_ascii=False, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


def validate_policy(policy: dict[str, Any]) -> None:
    required = {
        "schema_version", "environment", "canister_name", "canister_id",
        "stable_schema_version", "deployment_instance_id", "base_chain_id",
        "evm_rpc_canister_id", "before_module_sha256", "metadata_missing_module_sha256", "before_rpc_urls",
        "before_rpc_urls_sha256", "after_rpc_urls", "after_rpc_urls_sha256",
        "status_counts",
    }
    if set(policy) != required or policy["schema_version"] != 2:
        fail("RPC replacement policy has an unsupported shape")
    for field in ("before_module_sha256", "metadata_missing_module_sha256"):
        if not isinstance(policy[field], str) or not re.fullmatch(r"[0-9a-f]{64}", policy[field]):
            fail(f"policy {field} must be a lowercase SHA-256 digest")
    for side in ("before", "after"):
        urls = policy[f"{side}_rpc_urls"]
        if not isinstance(urls, list) or len(urls) != 3 or len(set(urls)) != 3:
            fail(f"policy {side} RPC list must contain three distinct URLs")
        if rpc_digest(urls) != policy[f"{side}_rpc_urls_sha256"]:
            fail(f"policy {side} RPC digest does not bind its ordered URL list")
    counts = policy["status_counts"]
    if not isinstance(counts, dict) or set(counts) != set(COUNT_FIELDS):
        fail("policy status_counts has an unsupported shape")
    for field in COUNT_FIELDS:
        value = counts[field]
        maximum = U128_MAX if field == "reserved_deposit_mint_amount" else U64_MAX
        if type(value) is not int or value < 0 or value > maximum:
            fail(f"policy status_counts.{field} is outside its Candid natural range")


def verify_clean_evidence(wasm: Path, did: Path, evidence: dict[str, Any]) -> str:
    dirty = run(["git", "status", "--porcelain", "--untracked-files=all"])
    if dirty.strip():
        fail("staging RPC replacement requires a clean checkout")
    head = run(["git", "rev-parse", "HEAD"]).strip()
    source = evidence.get("source_commit")
    if not isinstance(source, str) or not re.fullmatch(r"[0-9a-f]{40}", source):
        fail("local E2E evidence has no source commit")
    ancestry = subprocess.run(
        ["git", "merge-base", "--is-ancestor", source, head], cwd=ROOT, check=False
    )
    if ancestry.returncode:
        fail("local E2E source commit is not an ancestor of HEAD")
    changed = run(["git", "diff", "--name-only", source, head]).splitlines()
    allowed = {"deployments/sepolia-staging/evidence/local-e2e.json"}
    if source != head and set(changed) - allowed:
        fail("HEAD contains build-input changes after the local E2E source commit")
    if evidence.get("bridge_wasm_sha256") != sha256(wasm):
        fail("explicit Wasm does not match local E2E evidence")
    if evidence.get("candid_sha256") != sha256(did):
        fail("checked-in Candid does not match local E2E evidence")
    return head


def icp_base(policy: dict[str, Any], identity: str) -> list[str]:
    return ["-e", policy["environment"], "--identity", identity, "--project-root-override", str(ROOT)]


def call(policy: dict[str, Any], identity: str, did: Path, method: str) -> str:
    output = run([
        "icp", "canister", "call", policy["canister_name"], method, "()", "--query",
        "--candid", str(did), "--json", *icp_base(policy, identity),
    ])
    payload = json.loads(output)
    candid = payload.get("response_candid")
    if not isinstance(candid, str):
        fail(f"{method} did not return response_candid")
    return candid


def snapshot(policy: dict[str, Any], identity: str, did: Path) -> dict[str, Any]:
    public = call(policy, identity, did, "get_public_config")
    status = call(policy, identity, did, "get_bridge_status")
    integrity = call(policy, identity, did, "storage_integrity_check")
    status_raw = run([
        "icp", "canister", "status", policy["canister_name"], "--json",
        *icp_base(policy, identity),
    ])
    canister_status = json.loads(status_raw)
    module_hash = str(canister_status.get("module_hash", "")).lower().removeprefix("0x")
    if not re.fullmatch(r"[0-9a-f]{64}", module_hash):
        fail("canister status did not expose a module hash")
    return {
        "canister_id": run([
            "icp", "canister", "status", policy["canister_name"], "--id-only",
            *icp_base(policy, identity),
        ]).strip(),
        "schema_version": nat(public, "schema_version"),
        "deployment_instance_id": "0x" + blob(public, "deployment_instance_id", length=32).hex(),
        "base_chain_id": nat(public, "base_chain_id"),
        "evm_rpc_canister_id": principal(public, "evm_rpc_canister_id"),
        "rpc_provider_urls_sha256": blob(public, "rpc_provider_urls_sha256", length=32).hex(),
        "module_sha256": module_hash,
        "status_counts": {field: nat(status, field) for field in COUNT_FIELDS},
        "storage_integrity": "ok" if integrity_ok(integrity) else "failed",
    }


def verify_snapshot(snapshot_value: dict[str, Any], policy: dict[str, Any], *, phase: str, wasm_hash: str) -> None:
    expected = {
        "canister_id": policy["canister_id"],
        "schema_version": policy["stable_schema_version"],
        "deployment_instance_id": policy["deployment_instance_id"],
        "base_chain_id": policy["base_chain_id"],
        "evm_rpc_canister_id": policy["evm_rpc_canister_id"],
        "status_counts": policy["status_counts"],
        "storage_integrity": "ok",
    }
    if phase == "before":
        expected["module_sha256"] = policy["before_module_sha256"]
        expected["rpc_provider_urls_sha256"] = policy["before_rpc_urls_sha256"]
    elif phase == "metadata-missing":
        expected["module_sha256"] = policy["metadata_missing_module_sha256"]
        expected["rpc_provider_urls_sha256"] = policy["after_rpc_urls_sha256"]
    else:
        expected["module_sha256"] = wasm_hash
        expected["rpc_provider_urls_sha256"] = policy["after_rpc_urls_sha256"]
    for field, value in expected.items():
        if snapshot_value.get(field) != value:
            fail(f"{phase} snapshot {field} does not match the reviewed policy")


def live_public_metadata(ic_host: str, canister_id: str, name: str) -> str | None:
    output = run([
        "node", str(PUBLIC_METADATA_READER), ic_host, canister_id, name,
    ])
    try:
        payload = json.loads(output)
    except json.JSONDecodeError:
        fail(f"certified live canister lookup returned invalid {name} metadata JSON")
    if payload == {"status": "absent"}:
        return None
    if (
        isinstance(payload, dict)
        and set(payload) == {"status", "value"}
        and payload["status"] == "present"
        and isinstance(payload["value"], str)
    ):
        return payload["value"]
    fail(f"certified live canister lookup returned invalid {name} metadata status")


def live_private_metadata(policy: dict[str, Any], identity: str, name: str) -> str:
    output = run([
        "icp", "canister", "metadata", policy["canister_name"], name, "--json",
        *icp_base(policy, identity),
    ])
    try:
        payload = json.loads(output)
    except json.JSONDecodeError:
        fail(f"live canister returned invalid {name} metadata JSON")
    if (
        not isinstance(payload, dict)
        or set(payload) != {"value"}
        or not isinstance(payload["value"], str)
    ):
        fail(f"live canister returned invalid {name} metadata")
    return payload["value"]


def verify_candid_compatibility(policy: dict[str, Any], ic_host: str, did: Path) -> str | None:
    value = live_public_metadata(ic_host, policy["canister_id"], "candid:service")
    if value is None:
        return None
    with tempfile.NamedTemporaryFile("w", suffix=".did", encoding="utf-8") as previous:
        previous.write(value)
        previous.flush()
        run(["didc", "check", str(did), previous.name])
    return hashlib.sha256(value.encode()).hexdigest()


def verify_candidate_metadata(wasm: Path, did: Path) -> None:
    sections = run(["ic-wasm", str(wasm), "metadata"]).splitlines()
    if sections.count("icp:public candid:service") != 1:
        fail("explicit Wasm must contain one public candid:service metadata section")
    if sections.count("icp:private kinic:deployment") != 1:
        fail("explicit Wasm must contain one private kinic:deployment metadata section")
    candid = run(["ic-wasm", str(wasm), "metadata", "candid:service"]).removesuffix("\n")
    if candid.encode() != did.read_bytes():
        fail("explicit Wasm Candid metadata does not match the checked-in interface")
    if run(["ic-wasm", str(wasm), "metadata", "kinic:deployment"]).strip() != "test-deployment":
        fail("explicit Wasm deployment metadata is invalid")


def verify_live_metadata(policy: dict[str, Any], identity: str, ic_host: str, did: Path) -> str:
    candid = live_public_metadata(ic_host, policy["canister_id"], "candid:service")
    if candid is None:
        fail("live canister did not expose candid:service metadata")
    with tempfile.NamedTemporaryFile("w", suffix=".did", encoding="utf-8") as live:
        live.write(candid)
        live.flush()
        run(["didc", "check", str(did), live.name])
    if candid.encode() != did.read_bytes():
        fail("live canister Candid metadata does not match the checked-in interface")
    if live_private_metadata(policy, identity, "kinic:deployment") != "test-deployment":
        fail("live canister deployment metadata is invalid")
    return hashlib.sha256(candid.encode()).hexdigest()


def verify_provider_chains(policy: dict[str, Any]) -> None:
    for index, url in enumerate(policy["after_rpc_urls"]):
        observed = run(["cast", "chain-id", "--rpc-url", url]).strip()
        if observed != str(policy["base_chain_id"]):
            fail(f"staging RPC provider {index} returned an unexpected chain ID")


def install(policy: dict[str, Any], identity: str, wasm: Path) -> None:
    urls = "; ".join(json.dumps(url) for url in policy["after_rpc_urls"])
    counts = policy["status_counts"]
    count_fields = "; ".join(
        f"{field} = {counts[field]} : {'nat' if field == 'reserved_deposit_mint_amount' else 'nat64'}"
        for field in COUNT_FIELDS
    )
    args = (
        "(record { status_counts_guard_version = 1 : nat8; rpc_provider_update = opt record { "
        f"custom_evm_rpc_urls = vec {{ {urls} }}; "
        f"expected_status_counts = record {{ {count_fields} }}"
        " } })"
    )
    run([
        "icp", "canister", "install", policy["canister_name"], "--mode", "upgrade",
        "--wasm", str(wasm), "--args", args, "--yes", *icp_base(policy, identity),
    ], capture=False)


def write_evidence(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    temporary.replace(path)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--repair-missing-candid-metadata", action="store_true")
    parser.add_argument("--wasm", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    identity = os.environ.get("BRIDGE_STAGING_IDENTITY")
    if not identity:
        fail("BRIDGE_STAGING_IDENTITY is required")
    for tool in ("cast", "didc", "git", "ic-wasm", "icp", "node"):
        if shutil.which(tool) is None:
            fail(f"{tool} is required")
    wasm = args.wasm.resolve()
    evidence_path = args.evidence.resolve()
    if not args.wasm.is_absolute() or not wasm.is_file():
        fail("--wasm must name an existing absolute file")
    if not args.evidence.is_absolute():
        fail("--evidence must be an absolute path")
    policy = load_object(DEFAULT_POLICY, "RPC replacement policy")
    validate_policy(policy)
    profile = load_object(DEFAULT_PROFILE, "staging frontend profile")
    profile_binding = {
        "environment": policy["environment"],
        "bridgeCanisterId": policy["canister_id"],
        "deploymentInstanceId": policy["deployment_instance_id"],
        "chainId": policy["base_chain_id"],
        "evmRpcCanisterId": policy["evm_rpc_canister_id"],
    }
    for field, expected in profile_binding.items():
        if profile.get(field) != expected:
            fail(f"frontend profile {field} does not match the reviewed policy")
    ic_host = profile.get("icHost")
    if ic_host != IC_MAINNET_HOST:
        fail("frontend profile icHost does not use the reviewed IC mainnet endpoint")
    if [profile.get("baseRpcUrl"), *profile.get("baseHistoryRpcUrls", [])] != policy["after_rpc_urls"]:
        fail("frontend profile does not use the reviewed post-upgrade RPC order")
    if str(profile.get("rpcProviderUrlsSha256", "")).lower().removeprefix("0x") != policy["after_rpc_urls_sha256"]:
        fail("frontend profile does not bind the reviewed post-upgrade RPC digest")
    local_evidence = load_object(DEFAULT_LOCAL_EVIDENCE, "local E2E evidence")
    head = verify_clean_evidence(wasm, DEFAULT_DID, local_evidence)
    wasm_hash = sha256(wasm)
    did_hash = sha256(DEFAULT_DID)
    verify_candidate_metadata(wasm, DEFAULT_DID)
    before = snapshot(policy, identity, DEFAULT_DID)
    digest = before["rpc_provider_urls_sha256"]
    repair = (
        digest == policy["after_rpc_urls_sha256"]
        and before["module_sha256"] == policy["metadata_missing_module_sha256"]
    )
    if repair:
        if not args.repair_missing_candid_metadata:
            fail("known metadata-missing staging module requires the explicit repair flag")
        verify_snapshot(before, policy, phase="metadata-missing", wasm_hash=wasm_hash)
        candid_hash = verify_candid_compatibility(policy, ic_host, DEFAULT_DID)
        if candid_hash is not None:
            fail("metadata repair is allowed only when live candid:service metadata is absent")
        verify_provider_chains(policy)
        if not args.execute:
            print(json.dumps({"result": "metadata-repair-preflight-passed", "before": before}, sort_keys=True))
            return
        install(policy, identity, wasm)
        after = snapshot(policy, identity, DEFAULT_DID)
        verify_snapshot(after, policy, phase="after", wasm_hash=wasm_hash)
        after_candid_hash = verify_live_metadata(policy, identity, ic_host, DEFAULT_DID)
        result = "metadata-repaired"
    elif digest == policy["after_rpc_urls_sha256"]:
        verify_snapshot(before, policy, phase="after", wasm_hash=wasm_hash)
        candid_hash = verify_live_metadata(policy, identity, ic_host, DEFAULT_DID)
        verify_provider_chains(policy)
        if not args.execute:
            print(json.dumps({"result": "preflight-passed", "before": before}, sort_keys=True))
            return
        result = "already-applied"
        after = before
        after_candid_hash = candid_hash
    else:
        verify_snapshot(before, policy, phase="before", wasm_hash=wasm_hash)
        candid_hash = verify_live_metadata(policy, identity, ic_host, DEFAULT_DID)
        verify_provider_chains(policy)
        if not args.execute:
            print(json.dumps({"result": "preflight-passed", "before": before}, sort_keys=True))
            return
        install(policy, identity, wasm)
        after = snapshot(policy, identity, DEFAULT_DID)
        verify_snapshot(after, policy, phase="after", wasm_hash=wasm_hash)
        after_candid_hash = verify_live_metadata(policy, identity, ic_host, DEFAULT_DID)
        result = "upgraded"
    evidence = {
        "schema_version": 1,
        "kind": "staging-rpc-provider-replacement",
        "result": result,
        "source_commit": head,
        "local_e2e_source_commit": local_evidence["source_commit"],
        "wasm_sha256": wasm_hash,
        "candid_sha256": did_hash,
        "live_candid_sha256_before": candid_hash,
        "live_candid_sha256_after": after_candid_hash,
        "policy_sha256": sha256(DEFAULT_POLICY),
        "before": before,
        "after": after,
    }
    write_evidence(evidence_path, evidence)
    print(f"staging RPC provider replacement verified: {result}")


if __name__ == "__main__":
    main()
