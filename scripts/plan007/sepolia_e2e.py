#!/usr/bin/env python3
"""Build and verify the test-only IC mainnet × Base Sepolia E2E manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
KIND = "kinic-bridge-sepolia-staging-e2e"
CHAIN_ID = 84532
EVM_RPC_CANISTER_ID = "7hfb6-caaaa-aaaar-qadga-cai"
CURRENT_STABLE_SCHEMA = 25
STAGES = (
    "preflight",
    "install",
    "initialize",
    "contracts",
    "activation_schedule",
    "activation_execute",
    "frontend_publish",
    "wallet_e2e",
    "rpc_rehearsal",
    "final_pause",
)
RPC_SCENARIOS = {
    "preflight",
    "authorization_mint",
    "withdrawal_release",
    "ledger_fee_guard",
    "canonical_receipt",
    "single_provider_failure",
    "quorum_loss",
    "authorization_expiry",
    "processed_event_mismatch",
    "final_pause",
}
SHA256 = re.compile(r"^[0-9a-f]{64}$")
GIT_COMMIT = re.compile(r"^[0-9a-f]{40}$")
EVM_HASH = re.compile(r"^0x[0-9a-f]{64}$")
EVM_ADDRESS = re.compile(r"^0x[0-9a-f]{40}$")
PRINCIPAL = re.compile(r"^[a-z0-9-]{5,63}$")


class EvidenceError(ValueError):
    pass


def fail(message: str) -> None:
    raise EvidenceError(message)


def load_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read JSON object {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{path} must contain a JSON object")
    return value


def write_object(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def exact_keys(value: dict[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        fail(f"{context} fields differ: missing={sorted(expected - actual)} extra={sorted(actual - expected)}")


def require_string(value: dict[str, Any], name: str, context: str) -> str:
    result = value.get(name)
    if not isinstance(result, str) or not result:
        fail(f"{context}.{name} must be a non-empty string")
    return result


def require_pattern(value: dict[str, Any], name: str, pattern: re.Pattern[str], context: str) -> str:
    result = require_string(value, name, context)
    if not pattern.fullmatch(result):
        fail(f"{context}.{name} has an invalid format")
    return result


def require_nat(value: dict[str, Any], name: str, context: str) -> int:
    result = value.get(name)
    if isinstance(result, str) and result.isdigit():
        result = int(result)
    if not isinstance(result, int) or isinstance(result, bool) or result < 0:
        fail(f"{context}.{name} must be a natural number")
    return result


def require_bool(value: dict[str, Any], name: str, expected: bool, context: str) -> None:
    if value.get(name) is not expected:
        fail(f"{context}.{name} must be {str(expected).lower()}")


def validate_timestamp(value: str, context: str) -> None:
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        fail(f"{context} must be an ISO-8601 timestamp")
    if parsed.tzinfo is None:
        fail(f"{context} must include a timezone")


def validate_artifacts(artifacts: Any, manifest_path: Path, context: str, verify_files: bool) -> None:
    if not isinstance(artifacts, list) or not artifacts:
        fail(f"{context}.artifacts must be a non-empty array")
    for index, artifact in enumerate(artifacts):
        item_context = f"{context}.artifacts[{index}]"
        if not isinstance(artifact, dict):
            fail(f"{item_context} must be an object")
        exact_keys(artifact, {"path", "sha256", "kind"}, item_context)
        relative = Path(require_string(artifact, "path", item_context))
        if relative.is_absolute() or ".." in relative.parts:
            fail(f"{item_context}.path must stay below the manifest directory")
        require_pattern(artifact, "sha256", SHA256, item_context)
        require_string(artifact, "kind", item_context)
        if verify_files:
            target = manifest_path.parent / relative
            if not target.is_file():
                fail(f"{item_context}.path does not exist: {relative}")
            if digest(target) != artifact["sha256"]:
                fail(f"{item_context}.sha256 does not match {relative}")


def validate_preflight(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "preflight.details"
    exact_keys(
        details,
        {
            "chain_id",
            "evm_rpc_canister_id",
            "bridge_canister_id",
            "ledger_canister_id",
            "index_canister_id",
            "ledger_symbol",
            "ledger_decimals",
            "ledger_fee",
            "index_ledger_id",
            "controller_principals",
            "cycles_balance",
            "base_deposits_paused",
            "base_withdrawals_paused",
            "canister_deposits_paused",
            "configured_rpc_url_sha256",
        },
        context,
    )
    if details["chain_id"] != CHAIN_ID or details["evm_rpc_canister_id"] != EVM_RPC_CANISTER_ID:
        fail("preflight must use Base Sepolia and the official EVM RPC Canister")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        if details[field] != binding[field]:
            fail(f"{context}.{field} differs from the reviewed binding")
    if details["index_ledger_id"] != binding["ledger_canister_id"]:
        fail("preflight Index is not bound to the reviewed Ledger")
    require_string(details, "ledger_symbol", context)
    require_nat(details, "ledger_decimals", context)
    require_nat(details, "ledger_fee", context)
    require_nat(details, "cycles_balance", context)
    for field in ("base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused"):
        require_bool(details, field, True, context)
    controllers = details["controller_principals"]
    if not isinstance(controllers, list) or not controllers or any(not isinstance(item, str) or not PRINCIPAL.fullmatch(item) for item in controllers):
        fail("preflight controller_principals must contain explicit principals")
    urls = details["configured_rpc_url_sha256"]
    if not isinstance(urls, list) or len(urls) != 3 or len(set(urls)) != 3 or any(not isinstance(item, str) or not SHA256.fullmatch(item) for item in urls):
        fail("preflight must bind three distinct credential-free RPC URL digests")


def validate_install(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "install.details"
    exact_keys(details, {"install_mode", "module_sha256", "cycles_balance", "controller_principals"}, context)
    if details["install_mode"] not in {"install", "reinstall"}:
        fail("staging install_mode must be install or reinstall")
    if details["module_sha256"] != binding["bridge_wasm_sha256"]:
        fail("installed Bridge module does not match local promotion evidence")
    if require_nat(details, "cycles_balance", context) <= 0:
        fail("installed Bridge must retain a positive cycles balance")
    controllers = details["controller_principals"]
    if not isinstance(controllers, list) or not controllers:
        fail("install must retain an explicit controller set")


def validate_initialize(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "initialize.details"
    exact_keys(
        details,
        {
            "schema_version",
            "chain_id",
            "ledger_canister_id",
            "index_canister_id",
            "evm_rpc_canister_id",
            "expected_bridge_signer",
            "governance_operator",
            "canister_deposits_paused",
            "storage_integrity",
        },
        context,
    )
    if require_nat(details, "schema_version", context) != CURRENT_STABLE_SCHEMA:
        fail(f"staging must initialize current stable schema v{CURRENT_STABLE_SCHEMA}")
    if details["chain_id"] != CHAIN_ID:
        fail("initialized Bridge has the wrong chain ID")
    for field in ("ledger_canister_id", "index_canister_id"):
        if details[field] != binding[field]:
            fail(f"initialized {field} differs from the reviewed binding")
    if details["evm_rpc_canister_id"] != EVM_RPC_CANISTER_ID:
        fail("initialized Bridge does not use the official EVM RPC Canister")
    require_pattern(details, "expected_bridge_signer", EVM_ADDRESS, context)
    require_pattern(details, "governance_operator", EVM_ADDRESS, context)
    require_bool(details, "canister_deposits_paused", True, context)
    if details["storage_integrity"] != "ok":
        fail("storage_integrity_check did not return ok")


def validate_contracts(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "contracts.details"
    exact_keys(
        details,
        {
            "bridge_address",
            "bsns_address",
            "timelock_address",
            "bridge_runtime_hash",
            "bsns_runtime_hash",
            "deployment_block",
            "deployment_transaction_hashes",
            "mint_signer",
            "governance_operator",
            "deployer_roles_zero",
        },
        context,
    )
    for field in ("bridge_address", "bsns_address", "timelock_address", "mint_signer", "governance_operator"):
        require_pattern(details, field, EVM_ADDRESS, context)
    if details["bridge_runtime_hash"] != binding["bridge_runtime_hash"] or details["bsns_runtime_hash"] != binding["bsns_runtime_hash"]:
        fail("staging contract runtime hash differs from local promotion evidence")
    require_nat(details, "deployment_block", context)
    transactions = details["deployment_transaction_hashes"]
    if not isinstance(transactions, list) or not transactions or any(not isinstance(item, str) or not EVM_HASH.fullmatch(item) for item in transactions):
        fail("contracts must bind every deployment transaction")
    require_bool(details, "deployer_roles_zero", True, context)


def validate_activation_schedule(details: dict[str, Any]) -> None:
    context = "activation_schedule.details"
    exact_keys(details, {"operation_id", "schedule_transaction_hash", "finalized_block_number", "finalized_block_hash", "early_execute_reverted"}, context)
    require_pattern(details, "operation_id", EVM_HASH, context)
    require_pattern(details, "schedule_transaction_hash", EVM_HASH, context)
    require_nat(details, "finalized_block_number", context)
    require_pattern(details, "finalized_block_hash", EVM_HASH, context)
    require_bool(details, "early_execute_reverted", True, context)


def validate_activation_execute(details: dict[str, Any]) -> None:
    context = "activation_execute.details"
    exact_keys(
        details,
        {
            "delay_seconds",
            "execute_transaction_hash",
            "finalized_block_number",
            "finalized_block_hash",
            "base_deposits_paused",
            "base_withdrawals_paused",
            "canister_deposits_paused",
            "pending_timelock_operations",
        },
        context,
    )
    if require_nat(details, "delay_seconds", context) < 86400:
        fail("activation execute did not observe the full 24-hour delay")
    require_pattern(details, "execute_transaction_hash", EVM_HASH, context)
    require_nat(details, "finalized_block_number", context)
    require_pattern(details, "finalized_block_hash", EVM_HASH, context)
    for field in ("base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused"):
        require_bool(details, field, False, context)
    if require_nat(details, "pending_timelock_operations", context) != 0:
        fail("activation execute left a pending Timelock operation")


def validate_frontend(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "frontend_publish.details"
    exact_keys(details, {"url", "deployment_id", "profile_sha256", "test_banner_visible", "runtime_verification_fresh"}, context)
    if not require_string(details, "url", context).startswith("https://"):
        fail("staging frontend must use HTTPS")
    require_string(details, "deployment_id", context)
    if details["profile_sha256"] != binding["frontend_profile_sha256"]:
        fail("published frontend profile differs from the reviewed profile")
    require_bool(details, "test_banner_visible", True, context)
    require_bool(details, "runtime_verification_fresh", True, context)


def validate_wallet_flow(flow: Any, expected_wallet: str, context: str) -> None:
    if not isinstance(flow, dict):
        fail(f"{context} must be an object")
    exact_keys(
        flow,
        {
            "wallet",
            "operation_id",
            "ledger_block",
            "base_transaction_hash",
            "finalized_block_number",
            "finalized_block_hash",
            "completed",
        },
        context,
    )
    if flow["wallet"] != expected_wallet:
        fail(f"{context}.wallet must be {expected_wallet}")
    require_pattern(flow, "operation_id", EVM_HASH, context)
    require_nat(flow, "ledger_block", context)
    require_pattern(flow, "base_transaction_hash", EVM_HASH, context)
    require_nat(flow, "finalized_block_number", context)
    require_pattern(flow, "finalized_block_hash", EVM_HASH, context)
    require_bool(flow, "completed", True, context)


def validate_wallet_e2e(details: dict[str, Any]) -> None:
    context = "wallet_e2e.details"
    exact_keys(details, {"chrome_version", "wallet_versions", "deposits", "withdrawals", "walletconnect", "failure_checks", "same_wasm_upgrade"}, context)
    require_string(details, "chrome_version", context)
    versions = details["wallet_versions"]
    if not isinstance(versions, dict):
        fail("wallet_versions must be an object")
    exact_keys(versions, {"Plug", "OISY", "MetaMask", "Rabby", "WalletConnect"}, "wallet_versions")
    if any(not isinstance(value, str) or not value for value in versions.values()):
        fail("every wallet version must be recorded")
    deposits = details["deposits"]
    withdrawals = details["withdrawals"]
    if not isinstance(deposits, list) or len(deposits) != 2 or not isinstance(withdrawals, list) or len(withdrawals) != 2:
        fail("wallet E2E requires exactly two reviewed deposits and withdrawals")
    for flow, wallet in zip(deposits, ("Plug", "OISY"), strict=True):
        validate_wallet_flow(flow, wallet, f"wallet_e2e.deposits.{wallet}")
    for flow, wallet in zip(withdrawals, ("MetaMask", "Rabby"), strict=True):
        validate_wallet_flow(flow, wallet, f"wallet_e2e.withdrawals.{wallet}")
    walletconnect = details["walletconnect"]
    if not isinstance(walletconnect, dict):
        fail("walletconnect evidence must be an object")
    exact_keys(walletconnect, {"connected", "rejection_safe", "account_change_safe", "chain_change_safe", "csp_clean"}, "walletconnect")
    for field in walletconnect:
        require_bool(walletconnect, field, True, "walletconnect")
    checks = details["failure_checks"]
    required_checks = {
        "wallet_rejection",
        "popup_close",
        "reload",
        "duplicate_payload",
        "conflicting_payload",
        "sequence_gap",
        "two_tab_lease",
        "wallet_disconnect",
        "account_change",
        "chain_change",
        "runtime_mismatch",
        "notification_recovery",
    }
    if not isinstance(checks, dict):
        fail("failure_checks must be an object")
    exact_keys(checks, required_checks, "failure_checks")
    for field in required_checks:
        require_bool(checks, field, True, "failure_checks")
    upgrade = details["same_wasm_upgrade"]
    if not isinstance(upgrade, dict):
        fail("same_wasm_upgrade must be an object")
    exact_keys(upgrade, {"before_state_sha256", "after_state_sha256", "storage_integrity", "verified"}, "same_wasm_upgrade")
    before = require_pattern(upgrade, "before_state_sha256", SHA256, "same_wasm_upgrade")
    after = require_pattern(upgrade, "after_state_sha256", SHA256, "same_wasm_upgrade")
    if before != after or upgrade["storage_integrity"] != "ok":
        fail("same-Wasm upgrade did not preserve canonical state")
    require_bool(upgrade, "verified", True, "same_wasm_upgrade")


def validate_rpc(details: dict[str, Any]) -> None:
    context = "rpc_rehearsal.details"
    exact_keys(details, {"manifest_sha256", "state", "complete", "scenarios", "providers_restored"}, context)
    require_pattern(details, "manifest_sha256", SHA256, context)
    if details["state"] != "COMPLETE":
        fail("RPC rehearsal is not COMPLETE")
    require_bool(details, "complete", True, context)
    scenarios = details["scenarios"]
    if not isinstance(scenarios, list) or set(scenarios) != RPC_SCENARIOS or len(scenarios) != len(RPC_SCENARIOS):
        fail("RPC rehearsal does not bind the complete ten-scenario set")
    require_bool(details, "providers_restored", True, context)


def validate_final_pause(details: dict[str, Any]) -> None:
    context = "final_pause.details"
    exact_keys(
        details,
        {
            "base_deposits_paused",
            "base_withdrawals_paused",
            "canister_deposits_paused",
            "pending_timelock_operations",
            "pending_deposits",
            "pending_withdrawals",
            "providers_restored",
            "finalized_block_number",
            "finalized_block_hash",
        },
        context,
    )
    for field in ("base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused", "providers_restored"):
        require_bool(details, field, True, context)
    for field in ("pending_timelock_operations", "pending_deposits", "pending_withdrawals"):
        if require_nat(details, field, context) != 0:
            fail(f"final pause left nonzero {field}")
    require_nat(details, "finalized_block_number", context)
    require_pattern(details, "finalized_block_hash", EVM_HASH, context)


VALIDATORS = {
    "preflight": validate_preflight,
    "install": validate_install,
    "initialize": validate_initialize,
    "contracts": validate_contracts,
    "activation_schedule": validate_activation_schedule,
    "activation_execute": validate_activation_execute,
    "frontend_publish": validate_frontend,
    "wallet_e2e": validate_wallet_e2e,
    "rpc_rehearsal": validate_rpc,
    "final_pause": validate_final_pause,
}


def validate_binding(binding: Any) -> dict[str, Any]:
    if not isinstance(binding, dict):
        fail("binding must be an object")
    expected = {
        "source_commit",
        "local_e2e_sha256",
        "bridge_wasm_sha256",
        "bridge_runtime_hash",
        "bsns_runtime_hash",
        "frontend_profile_sha256",
        "bridge_canister_id",
        "ledger_canister_id",
        "index_canister_id",
    }
    exact_keys(binding, expected, "binding")
    require_pattern(binding, "source_commit", GIT_COMMIT, "binding")
    for field in ("local_e2e_sha256", "bridge_wasm_sha256", "frontend_profile_sha256"):
        require_pattern(binding, field, SHA256, "binding")
    for field in ("bridge_runtime_hash", "bsns_runtime_hash"):
        require_pattern(binding, field, EVM_HASH, "binding")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        require_pattern(binding, field, PRINCIPAL, "binding")
    return binding


def validate_stage(stage: str, evidence: Any, manifest_path: Path, binding: dict[str, Any], verify_files: bool) -> None:
    if not isinstance(evidence, dict):
        fail(f"{stage} evidence must be an object")
    exact_keys(evidence, {"schema_version", "stage", "observed_at", "source_commit", "artifacts", "details"}, f"{stage} evidence")
    if evidence["schema_version"] != SCHEMA_VERSION or evidence["stage"] != stage:
        fail(f"{stage} evidence has the wrong schema or stage")
    if evidence["source_commit"] != binding["source_commit"]:
        fail(f"{stage} evidence is not bound to the reviewed source commit")
    validate_timestamp(require_string(evidence, "observed_at", f"{stage} evidence"), f"{stage}.observed_at")
    validate_artifacts(evidence["artifacts"], manifest_path, stage, verify_files)
    details = evidence["details"]
    if not isinstance(details, dict):
        fail(f"{stage}.details must be an object")
    validator = VALIDATORS[stage]
    if stage in {"preflight", "install", "initialize", "contracts", "frontend_publish"}:
        validator(details, binding)
    else:
        validator(details)


def validate_manifest(manifest: dict[str, Any], path: Path, *, require_complete: bool, verify_files: bool) -> None:
    exact_keys(manifest, {"schema_version", "kind", "state", "created_at", "updated_at", "binding", "stages", "complete"}, "manifest")
    if manifest["schema_version"] != SCHEMA_VERSION or manifest["kind"] != KIND:
        fail("manifest has an unsupported schema or kind")
    validate_timestamp(require_string(manifest, "created_at", "manifest"), "manifest.created_at")
    validate_timestamp(require_string(manifest, "updated_at", "manifest"), "manifest.updated_at")
    binding = validate_binding(manifest["binding"])
    stages = manifest["stages"]
    if not isinstance(stages, dict) or set(stages) != set(STAGES):
        fail("manifest stages must use the fixed Plan 007 order")
    completed: list[str] = []
    encountered_gap = False
    for stage in STAGES:
        evidence = stages[stage]
        if evidence is None:
            encountered_gap = True
            continue
        if encountered_gap:
            fail("manifest cannot skip an earlier stage")
        validate_stage(stage, evidence, path, binding, verify_files)
        completed.append(stage)
    expected_state = "COMPLETE" if len(completed) == len(STAGES) else f"AWAITING_{STAGES[len(completed)].upper()}"
    expected_complete = len(completed) == len(STAGES)
    if manifest["state"] != expected_state or manifest["complete"] is not expected_complete:
        fail("manifest state does not match its recorded stages")
    if require_complete and not expected_complete:
        fail(f"staging E2E is incomplete: {manifest['state']}")


def initialize(output: Path, local_evidence_path: Path, profile_path: Path, repo_root: Path | None = None) -> None:
    if output.exists():
        fail(f"refusing to overwrite existing manifest: {output}")
    local = load_object(local_evidence_path)
    profile = load_object(profile_path)
    required_tests = {"full_local_ci", "real_frontend_e2e", "canister_activation", "timelock_24h", "state_upgrade"}
    if local.get("schema_version") != 3 or set(local.get("tests", {})) != required_tests or any(local["tests"][name] != "passed" for name in required_tests):
        fail("local promotion evidence is not a complete schema v3 pass")
    if profile.get("environment") != "sepolia-staging" or profile.get("testOnly") is not True or profile.get("chainId") != CHAIN_ID:
        fail("frontend profile is not the Base Sepolia test-only profile")
    if profile.get("evmRpcCanisterId") != EVM_RPC_CANISTER_ID:
        fail("frontend profile does not use the official EVM RPC Canister")
    if repo_root is not None:
        head = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=repo_root,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        status = subprocess.run(
            ["git", "status", "--porcelain"],
            cwd=repo_root,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        if head.returncode != 0 or status.returncode != 0:
            fail("cannot verify the Plan 007 source worktree")
        dirty_paths = [line for line in status.stdout.splitlines() if line.strip()]
        allowed_local_evidence = local_evidence_path.resolve()
        unexpected = []
        for line in dirty_paths:
            relative = line[3:].strip()
            candidate = (repo_root / relative).resolve()
            if candidate != allowed_local_evidence or line[:2] not in {" M", "M "}:
                unexpected.append(line)
        if unexpected:
            fail("staging E2E initialization allows only the freshly generated local-e2e.json diff")
        if local.get("source_commit") != head.stdout.strip():
            fail("local promotion evidence is stale for the current source commit")
    binding = {
        "source_commit": local.get("source_commit"),
        "local_e2e_sha256": digest(local_evidence_path),
        "bridge_wasm_sha256": local.get("bridge_wasm_sha256"),
        "bridge_runtime_hash": local.get("bridge_runtime_hash"),
        "bsns_runtime_hash": local.get("bsns_runtime_hash"),
        "frontend_profile_sha256": digest(profile_path),
        "bridge_canister_id": profile.get("bridgeCanisterId"),
        "ledger_canister_id": profile.get("ledgerCanisterId"),
        "index_canister_id": profile.get("indexCanisterId"),
    }
    validate_binding(binding)
    timestamp = now()
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "state": "AWAITING_PREFLIGHT",
        "created_at": timestamp,
        "updated_at": timestamp,
        "binding": binding,
        "stages": {stage: None for stage in STAGES},
        "complete": False,
    }
    write_object(output, manifest)


def record(manifest_path: Path, evidence_path: Path) -> None:
    manifest = load_object(manifest_path)
    validate_manifest(manifest, manifest_path, require_complete=False, verify_files=False)
    evidence = load_object(evidence_path)
    stage = evidence.get("stage")
    if stage not in STAGES:
        fail("evidence stage is not part of Plan 007")
    expected = next((name for name in STAGES if manifest["stages"][name] is None), None)
    if stage != expected:
        fail(f"expected stage {expected}, received {stage}")
    validate_stage(stage, evidence, manifest_path, validate_binding(manifest["binding"]), verify_files=True)
    manifest["stages"][stage] = evidence
    manifest["updated_at"] = now()
    next_stage = next((name for name in STAGES if manifest["stages"][name] is None), None)
    manifest["complete"] = next_stage is None
    manifest["state"] = "COMPLETE" if next_stage is None else f"AWAITING_{next_stage.upper()}"
    write_object(manifest_path, manifest)


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    init_parser = subparsers.add_parser("init")
    init_parser.add_argument("manifest", type=Path)
    init_parser.add_argument("local_e2e", type=Path)
    init_parser.add_argument("frontend_profile", type=Path)
    init_parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    record_parser = subparsers.add_parser("record")
    record_parser.add_argument("manifest", type=Path)
    record_parser.add_argument("evidence", type=Path)
    verify_parser = subparsers.add_parser("verify")
    verify_parser.add_argument("manifest", type=Path)
    verify_parser.add_argument("--allow-incomplete", action="store_true")
    args = parser.parse_args()
    try:
        if args.command == "init":
            initialize(args.manifest, args.local_e2e, args.frontend_profile, args.repo_root)
            print(f"initialized {args.manifest}")
        elif args.command == "record":
            record(args.manifest, args.evidence)
            manifest = load_object(args.manifest)
            print(f"recorded {args.evidence}; state={manifest['state']}")
        else:
            manifest = load_object(args.manifest)
            validate_manifest(manifest, args.manifest, require_complete=not args.allow_incomplete, verify_files=True)
            print(f"verified {args.manifest}; state={manifest['state']}")
    except EvidenceError as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
