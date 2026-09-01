#!/usr/bin/env python3
"""Build and verify the test-only IC mainnet × Base Sepolia E2E manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 8
KIND = "kinic-bridge-sepolia-staging-e2e"
CHAIN_ID = 84532
ENVIRONMENT_MODE = "short-delay-test-only"
ACTIVATION_TIMELOCK_DELAY_SECONDS = 300
EVM_RPC_CANISTER_ID = "7hfb6-caaaa-aaaar-qadga-cai"
CURRENT_STABLE_SCHEMA = 35
CURRENT_RECORD_WIRE_VERSION = 30
LIVE_PUBLIC_CONFIG_ARTIFACT_KIND = "live-public-config"
UPGRADE_INSTANCE_CHECK_ARTIFACT_KIND = "upgrade-instance-check"
WITHDRAWAL_BOUNDARY_ARTIFACT_KIND = "withdrawal-admission-boundary"
LIVE_BRIDGE_STATUS_ARTIFACT_KIND = "live-bridge-status"
LIVE_ACTIVATION_STATUS_ARTIFACT_KIND = "live-activation-status"
LIVE_CANISTER_STATUS_ARTIFACT_KIND = "live-canister-status"
LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND = "live-storage-integrity"
LIVE_LEDGER_BALANCE_ARTIFACT_KIND = "live-ledger-balance"
LIVE_LEDGER_METADATA_ARTIFACT_KIND = "live-ledger-metadata"
HISTORICAL_REINSTALL_DECISION_ARTIFACT_KIND = "historical-reinstall-decision"
HISTORICAL_FRESH_STACK_ARTIFACT_KIND = "historical-fresh-stack"
RPC_REHEARSAL_MANIFEST_ARTIFACT_KIND = "rpc-rehearsal-manifest"
REACTIVATION_SCHEDULE_RECEIPT_ARTIFACT_KIND = "reactivation-schedule-receipt"
REACTIVATION_EXECUTE_RECEIPT_ARTIFACT_KIND = "reactivation-execute-receipt"
STAGING_MONITORING_RECEIPT_ARTIFACT_KIND = "staging-monitoring-receipt"
TRUSTED_HISTORICAL_ARTIFACTS = {
    HISTORICAL_REINSTALL_DECISION_ARTIFACT_KIND: (
        "reinstall-decision-2026-08-27.json",
        "a458b8eff29c2cd6d5311466480b8c84902ec4d9f89326825f3a9514da308021",
    ),
    HISTORICAL_FRESH_STACK_ARTIFACT_KIND: (
        "fresh-stack-2026-08-28.json",
        "870177582b08f40424912f89acbcb2aa3be8790f08109c4705e6d515b610bcad",
    ),
}
CURRENT_SCHEMA_UPGRADE = "current-schema-upgrade"
UPGRADE_STATE_COUNT_FIELDS = {
    "deposits",
    "withdrawals",
    "pending_ledger_operations",
    "reconciliation_holds",
    "reserved_deposit_mint_operations",
    "reserved_deposit_mint_amount",
    "unpaid_withdrawal_count",
    "unpaid_withdrawal_amount_out",
}
STAGES = (
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
)
STAGE_RECEIPT_KINDS = {
    stage: f"{stage}-receipt"
    for stage in (
        "current_schema_upgrade",
        "post_upgrade_binding",
        "frontend_publish",
        "smoke_e2e",
        "wallet_e2e",
        "refund_rehearsal",
    )
}
STAGE_RAW_CAPTURE_KINDS = {
    stage: f"{stage}-raw-capture" for stage in STAGE_RECEIPT_KINDS
}
LIVE_ZERO_COUNT_FIELDS = {
    "pending_governance_operations",
    "pending_timelock_operations",
    "pending_deposits",
    "pending_withdrawals",
    "pending_reconciliation_jobs",
    "pending_ledger_operations",
    "reserved_deposit_mint_operations",
    "reserved_deposit_mint_amount",
    "unpaid_withdrawal_count",
    "unpaid_withdrawal_amount_out",
}
LIVE_MONITOR_FIELDS = {
    "initial_activation_operation_id",
    "reactivation_operation_id",
    "reactivation_schedule_transaction_hash",
    "reactivation_execute_transaction_hash",
    "reactivation_delay_seconds",
    "base_deposits_paused",
    "base_withdrawals_paused",
    "canister_deposits_paused",
    *LIVE_ZERO_COUNT_FIELDS,
    "providers_restored",
    "settlement_scheduler_healthy",
    "storage_integrity",
    "reserve_sufficient",
    "schema_version",
    "record_wire_version",
    "deployment_instance_id",
    "minimum_withdrawal_id",
    "bridge_canister_id",
    "module_sha256",
    "candid_sha256",
    "frontend_profile_sha256",
    "bridge_runtime_sha256",
    "mint_authorization_ttl_seconds",
    "solidity_max_authorization_horizon_seconds",
    "old_stack_excluded",
    "finalized_block_number",
    "finalized_block_hash",
}
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


def object_digest(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


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


def require_deployment_instance_id(value: dict[str, Any], name: str, context: str) -> str:
    result = require_pattern(value, name, EVM_HASH, context)
    if int(result[2:], 16) == 0:
        fail(f"{context}.{name} must be nonzero")
    return result


def deployment_instance_hex(value: Any, context: str) -> str:
    if isinstance(value, str) and re.fullmatch(r"0x[0-9a-fA-F]{64}", value):
        normalized = value.lower()
        if int(normalized[2:], 16) != 0:
            return normalized
    if (
        isinstance(value, list)
        and len(value) == 32
        and any(byte != 0 for byte in value)
        and all(
            isinstance(byte, int)
            and not isinstance(byte, bool)
            and 0 <= byte <= 255
            for byte in value
        )
    ):
        return "0x" + bytes(value).hex()
    fail(f"{context} must be a nonzero 32-byte deployment instance ID")


def digest_hex(value: Any, context: str) -> str:
    if isinstance(value, str):
        normalized = value.lower()
        if re.fullmatch(r"(?:0x)?[0-9a-f]{64}", normalized):
            return normalized if normalized.startswith("0x") else f"0x{normalized}"
    if (
        isinstance(value, list)
        and len(value) == 32
        and all(
            isinstance(byte, int)
            and not isinstance(byte, bool)
            and 0 <= byte <= 255
            for byte in value
        )
    ):
        return "0x" + bytes(value).hex()
    fail(f"{context} must be a 32-byte SHA-256 digest")


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


def validate_timestamp(value: str, context: str) -> datetime:
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        fail(f"{context} must be an ISO-8601 timestamp")
    if parsed.tzinfo is None:
        fail(f"{context} must include a timezone")
    return parsed


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


def required_artifact_path(
    artifacts: Any,
    manifest_path: Path,
    kind: str,
    context: str,
) -> tuple[dict[str, Any], Path]:
    matches = [
        artifact
        for artifact in artifacts
        if isinstance(artifact, dict) and artifact.get("kind") == kind
    ]
    if len(matches) != 1:
        fail(f"{context}.artifacts must contain exactly one {kind} artifact")
    artifact = matches[0]
    relative = Path(require_string(artifact, "path", f"{context}.{kind}"))
    target = manifest_path.parent / relative
    if not target.is_file():
        fail(f"{context}.{kind} artifact does not exist: {relative}")
    if digest(target) != artifact["sha256"]:
        fail(f"{context}.{kind} artifact sha256 does not match {relative}")
    return artifact, target


def required_json_artifact(
    artifacts: Any,
    manifest_path: Path,
    kind: str,
    context: str,
) -> dict[str, Any]:
    _, target = required_artifact_path(artifacts, manifest_path, kind, context)
    return load_object(target)


def validate_stage_receipt(
    stage: str,
    evidence: dict[str, Any],
    manifest_path: Path,
) -> None:
    kind = STAGE_RECEIPT_KINDS[stage]
    capture_kind = STAGE_RAW_CAPTURE_KINDS[stage]
    context = f"{stage}.{kind}"
    receipt = required_json_artifact(
        evidence["artifacts"], manifest_path, kind, stage
    )
    exact_keys(
        receipt,
        {
            "schema_version",
            "kind",
            "stage",
            "observed_at",
            "source_commit",
            "details_sha256",
            "capture_sha256",
        },
        context,
    )
    if receipt["schema_version"] != SCHEMA_VERSION or receipt["kind"] != kind:
        fail(f"{context} has the wrong schema or kind")
    if (
        receipt["stage"] != stage
        or receipt["observed_at"] != evidence["observed_at"]
        or receipt["source_commit"] != evidence["source_commit"]
    ):
        fail(f"{context} is not bound to its stage evidence")
    if require_pattern(receipt, "details_sha256", SHA256, context) != object_digest(
        evidence["details"]
    ):
        fail(f"{context} does not bind the recorded details")
    capture_descriptor, capture_path = required_artifact_path(
        evidence["artifacts"], manifest_path, capture_kind, stage
    )
    if require_pattern(receipt, "capture_sha256", SHA256, context) != capture_descriptor["sha256"]:
        fail(f"{context} does not bind the raw stage capture")
    capture = load_object(capture_path)
    capture_context = f"{stage}.{capture_kind}"
    exact_keys(
        capture,
        {
            "schema_version",
            "kind",
            "stage",
            "observed_at",
            "source_commit",
            "tool",
            "argv",
            "exit_code",
            "stdout",
            "stdout_sha256",
        },
        capture_context,
    )
    if capture["schema_version"] != 1 or capture["kind"] != capture_kind:
        fail(f"{capture_context} has the wrong schema or kind")
    if (
        capture["stage"] != stage
        or capture["observed_at"] != evidence["observed_at"]
        or capture["source_commit"] != evidence["source_commit"]
    ):
        fail(f"{capture_context} is not bound to its stage evidence")
    tool = require_string(capture, "tool", capture_context)
    argv = capture["argv"]
    if (
        not isinstance(argv, list)
        or not argv
        or any(not isinstance(argument, str) or not argument for argument in argv)
        or argv[0] != tool
    ):
        fail(f"{capture_context}.argv must identify the capture tool")
    if capture["exit_code"] != 0:
        fail(f"{capture_context}.exit_code must be zero")
    stdout = capture.get("stdout")
    if not isinstance(stdout, str) or not stdout:
        fail(f"{capture_context}.stdout must contain the captured JSON result")
    stdout_sha256 = hashlib.sha256(stdout.encode()).hexdigest()
    if require_pattern(capture, "stdout_sha256", SHA256, capture_context) != stdout_sha256:
        fail(f"{capture_context}.stdout_sha256 does not match stdout")
    try:
        parsed = json.loads(stdout)
    except json.JSONDecodeError as error:
        fail(f"{capture_context}.stdout is not JSON: {error}")
    if parsed != evidence["details"]:
        fail(f"{capture_context}.stdout does not reproduce the recorded details")


def upgrade_instance_check(
    next_deployment_instance_id: Any,
    expected_rpc_provider_urls_sha256: Any,
    live_public_config: dict[str, Any],
    live_canister_status: dict[str, Any],
    bridge_canister_id: str,
) -> dict[str, Any]:
    exact_keys(
        live_canister_status,
        {
            "canister_id",
            "module_hash",
            "controller_principals",
            "cycles_balance",
        },
        "live canister status",
    )
    if live_canister_status["canister_id"] != bridge_canister_id:
        fail("live canister status is not bound to the reviewed Bridge canister")
    next_id = deployment_instance_hex(
        next_deployment_instance_id,
        "binding deployment_instance_id",
    )
    schema_version = require_nat(
        live_public_config,
        "schema_version",
        "live RuntimeBinding",
    )
    if schema_version != CURRENT_STABLE_SCHEMA:
        fail(f"staging upgrade requires current stable schema v{CURRENT_STABLE_SCHEMA}")
    previous = deployment_instance_hex(
        live_public_config.get("deployment_instance_id"),
        "live RuntimeBinding deployment_instance_id",
    )
    live_module_hash = require_pattern(
        live_canister_status,
        "module_hash",
        EVM_HASH,
        "live canister status",
    )
    if live_module_hash == "0x" + "0" * 64:
        fail("live canister status module_hash must be nonzero")
    if previous != next_id:
        fail("reinstall is prohibited: staging upgrade must preserve the deployment instance ID")
    if digest_hex(
        live_public_config.get("rpc_provider_urls_sha256"),
        "live RuntimeBinding rpc_provider_urls_sha256",
    ) != digest_hex(
        expected_rpc_provider_urls_sha256,
        "binding rpc_provider_urls_sha256",
    ):
        fail("live RuntimeBinding RPC provider digest differs from the reviewed profile")
    return {
        "replacement_mode": CURRENT_SCHEMA_UPGRADE,
        "live_schema_version": schema_version,
        "previous_deployment_instance_id": previous,
        "live_module_hash": live_module_hash,
        "next": next_id,
    }


def normalized_upgrade_check(value: dict[str, Any]) -> dict[str, Any]:
    context = "upgrade instance check"
    exact_keys(
        value,
        {
            "replacement_mode",
            "live_schema_version",
            "previous_deployment_instance_id",
            "live_module_hash",
            "next",
        },
        context,
    )
    schema_version = require_nat(value, "live_schema_version", context)
    replacement_mode = require_string(value, "replacement_mode", context)
    previous = deployment_instance_hex(
        value["previous_deployment_instance_id"],
        f"{context} previous_deployment_instance_id",
    )
    next_id = deployment_instance_hex(value["next"], f"{context} next")
    if schema_version != CURRENT_STABLE_SCHEMA or previous != next_id:
        fail(f"{context} must preserve the current schema and deployment instance ID")
    if replacement_mode != CURRENT_SCHEMA_UPGRADE:
        fail(f"{context} replacement_mode must be {CURRENT_SCHEMA_UPGRADE}")
    return {
        "replacement_mode": replacement_mode,
        "live_schema_version": schema_version,
        "previous_deployment_instance_id": previous,
        "live_module_hash": require_pattern(
            value,
            "live_module_hash",
            EVM_HASH,
            context,
        ),
        "next": next_id,
    }


def validate_withdrawal_boundary(
    value: dict[str, Any],
    binding: dict[str, Any],
    preflight_observed_at: str,
    configured_provider_digests: list[str],
) -> str:
    context = "withdrawal admission boundary"
    exact_keys(value, {
        "schema_version", "kind", "observed_at", "chain_id", "bridge_address",
        "finalized_checkpoint_block_number", "finalized_checkpoint_block_hash",
        "withdrawals_paused", "minimum_withdrawal_id", "providers",
    }, context)
    if value["schema_version"] != 1 or value["kind"] != WITHDRAWAL_BOUNDARY_ARTIFACT_KIND:
        fail(f"{context} has an unsupported schema or kind")
    boundary_observed_at = validate_timestamp(
        require_string(value, "observed_at", context),
        f"{context}.observed_at",
    )
    preflight_at = validate_timestamp(preflight_observed_at, "preflight.observed_at")
    boundary_age = preflight_at - boundary_observed_at
    if boundary_age < timedelta(0) or boundary_age > timedelta(minutes=5):
        fail(f"{context} must be observed no more than five minutes before preflight")
    if require_nat(value, "chain_id", context) != binding["chain_id"]:
        fail(f"{context} chain ID differs from the binding")
    if require_pattern(value, "bridge_address", EVM_ADDRESS, context).lower() != binding["bridge_address"].lower():
        fail(f"{context} Bridge address differs from the binding")
    require_nat(value, "finalized_checkpoint_block_number", context)
    checkpoint_hash = require_pattern(value, "finalized_checkpoint_block_hash", EVM_HASH, context)
    require_bool(value, "withdrawals_paused", False, context)
    minimum = require_deployment_instance_id(value, "minimum_withdrawal_id", context)
    providers = value["providers"]
    if not isinstance(providers, list) or not 2 <= len(providers) <= 3:
        fail(f"{context} requires two or three eligible provider observations")
    agreeing = 0
    digests: list[str] = []
    for index, provider in enumerate(providers):
        provider_context = f"{context}.providers[{index}]"
        if not isinstance(provider, dict):
            fail(f"{provider_context} must be an object")
        exact_keys(provider, {
            "provider_url_sha256", "finalized_head_block_number", "checkpoint_block_number", "checkpoint_block_hash",
            "withdrawals_paused", "minimum_withdrawal_id",
        }, provider_context)
        digests.append(require_pattern(provider, "provider_url_sha256", SHA256, provider_context))
        require_bool(provider, "withdrawals_paused", False, provider_context)
        head = require_nat(provider, "finalized_head_block_number", provider_context)
        checkpoint = value["finalized_checkpoint_block_number"]
        if head < checkpoint:
            fail(f"{provider_context}.finalized_head_block_number is below the median checkpoint")
        provider_minimum = require_deployment_instance_id(provider, "minimum_withdrawal_id", provider_context)
        if (
            require_nat(provider, "checkpoint_block_number", provider_context)
            == value["finalized_checkpoint_block_number"]
            and require_pattern(provider, "checkpoint_block_hash", EVM_HASH, provider_context) == checkpoint_hash
            and provider_minimum == minimum
        ):
            agreeing += 1
    expected_digests = [digest for digest in configured_provider_digests if digest in digests]
    if digests != expected_digests:
        fail(f"{context} provider order differs from the configured RPC providers")
    if len(set(digests)) != len(digests) or agreeing < 2:
        fail(f"{context} does not contain a two-provider canonical quorum")
    if minimum != binding["minimum_withdrawal_id"]:
        fail(f"{context} minimum withdrawal ID differs from the reviewed binding")
    return minimum


def validate_upgrade_snapshots(
    artifacts: Any,
    manifest_path: Path,
) -> None:
    bridge_status = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_BRIDGE_STATUS_ARTIFACT_KIND,
        "upgrade preflight",
    )
    exact_keys(bridge_status, UPGRADE_STATE_COUNT_FIELDS, "live bridge status")
    for field in UPGRADE_STATE_COUNT_FIELDS:
        require_nat(bridge_status, field, "live bridge status")

    activation = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_ACTIVATION_STATUS_ARTIFACT_KIND,
        "upgrade preflight",
    )
    exact_keys(activation, {"pending_timelock_operations"}, "live activation status")
    if require_nat(
        activation,
        "pending_timelock_operations",
        "live activation status",
    ) != 0:
        fail("upgrade requires zero pending Timelock operations")

    integrity = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND,
        "upgrade preflight",
    )
    exact_keys(integrity, {"result"}, "live storage integrity")
    if integrity["result"] != "ok":
        fail("upgrade requires storage_integrity_check to return ok")

    ledger = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_LEDGER_BALANCE_ARTIFACT_KIND,
        "upgrade preflight",
    )
    exact_keys(ledger, {"balance_raw"}, "live ledger balance")
    require_nat(ledger, "balance_raw", "live ledger balance")


def validate_bootstrap_attestation(
    details: dict[str, Any],
    binding: dict[str, Any],
    artifacts: Any,
    manifest_path: Path,
) -> None:
    context = "bootstrap_attestation.details"
    exact_keys(
        details,
        {
            "historical_reinstall_completed",
            "historical_reinstall_resumable",
            "future_update_mode",
            "bridge_canister_id",
            "deployment_instance_id",
            "stable_schema_version",
        },
        context,
    )
    require_bool(details, "historical_reinstall_completed", True, context)
    require_bool(details, "historical_reinstall_resumable", False, context)
    if require_string(details, "future_update_mode", context) != CURRENT_SCHEMA_UPGRADE:
        fail("bootstrap attestation must permit only current-schema upgrades")
    if details["bridge_canister_id"] != binding["bridge_canister_id"]:
        fail("historical reinstall used a different Bridge Canister")
    if require_deployment_instance_id(
        details, "deployment_instance_id", context
    ) != binding["deployment_instance_id"]:
        fail("historical fresh stack has a different deployment instance")
    if require_nat(details, "stable_schema_version", context) != CURRENT_STABLE_SCHEMA:
        fail("historical fresh stack does not use the current stable schema")

    for kind, (expected_path, expected_sha256) in TRUSTED_HISTORICAL_ARTIFACTS.items():
        matches = [
            artifact
            for artifact in artifacts
            if isinstance(artifact, dict) and artifact.get("kind") == kind
        ]
        if len(matches) != 1:
            fail(f"bootstrap attestation must contain exactly one trusted {kind}")
        if (
            matches[0].get("path") != expected_path
            or matches[0].get("sha256") != expected_sha256
        ):
            fail(f"bootstrap attestation {kind} is not the pinned historical artifact")

    decision = required_json_artifact(
        artifacts,
        manifest_path,
        HISTORICAL_REINSTALL_DECISION_ARTIFACT_KIND,
        "bootstrap attestation",
    )
    fresh = required_json_artifact(
        artifacts,
        manifest_path,
        HISTORICAL_FRESH_STACK_ARTIFACT_KIND,
        "bootstrap attestation",
    )
    if (
        decision.get("schema_version") != 1
        or decision.get("kind") != "staging-reinstall-decision"
        or decision.get("decision")
        != "reuse-canister-principal-by-destructive-reinstall"
        or decision.get("bridge_canister_id") != binding["bridge_canister_id"]
    ):
        fail("historical reinstall decision is not the reviewed one-time exception")
    if (
        fresh.get("schema_version") != 1
        or fresh.get("kind") != "fresh-staging-stack"
        or fresh.get("stable_schema_version") != CURRENT_STABLE_SCHEMA
    ):
        fail("historical fresh-stack evidence has an unsupported schema or kind")
    reuse = fresh.get("reuse")
    created = fresh.get("create")
    acceptance = fresh.get("acceptance")
    if not isinstance(reuse, dict) or not isinstance(created, dict) or not isinstance(acceptance, dict):
        fail("historical fresh-stack evidence is incomplete")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        if reuse.get(field) != binding[field]:
            fail(f"historical fresh-stack {field} differs from the active binding")
    for field in (
        "deployment_instance_id",
        "timelock_address",
        "bridge_address",
        "bsns_address",
        "expected_bridge_signer",
    ):
        if str(created.get(field, "")).lower() != str(binding[field]).lower():
            fail(f"historical fresh-stack {field} differs from the active binding")
    if acceptance.get("prior_canister_test_state_discarded_by_reinstall") is not True:
        fail("historical fresh-stack evidence does not acknowledge discarded test state")


def validate_preflight(
    details: dict[str, Any],
    binding: dict[str, Any],
    artifacts: Any,
    manifest_path: Path,
    preflight_observed_at: str,
) -> None:
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
            "replacement_mode",
            "live_schema_version",
            "previous_deployment_instance_id",
            "minimum_withdrawal_id",
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
    if require_string(details, "ledger_symbol", context) != "TICRC1":
        fail("preflight must use the reviewed TICRC1 Ledger symbol")
    if require_nat(details, "ledger_decimals", context) != 8:
        fail("preflight must use the reviewed TICRC1 Ledger decimals")
    if require_nat(details, "ledger_fee", context) != 10_000:
        fail("preflight must use the test-deployment Ledger fee 10000")
    require_nat(details, "cycles_balance", context)
    for field in ("base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused"):
        require_bool(details, field, False, context)
    controllers = details["controller_principals"]
    if not isinstance(controllers, list) or not controllers or any(not isinstance(item, str) or not PRINCIPAL.fullmatch(item) for item in controllers):
        fail("preflight controller_principals must contain explicit principals")
    urls = details["configured_rpc_url_sha256"]
    if not isinstance(urls, list) or len(urls) != 3 or len(set(urls)) != 3 or any(not isinstance(item, str) or not SHA256.fullmatch(item) for item in urls):
        fail("preflight must bind three distinct credential-free RPC URL digests")
    live_public_config = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_PUBLIC_CONFIG_ARTIFACT_KIND,
        "preflight",
    )
    recorded_check = required_json_artifact(
        artifacts,
        manifest_path,
        UPGRADE_INSTANCE_CHECK_ARTIFACT_KIND,
        "preflight",
    )
    live_canister_status = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_CANISTER_STATUS_ARTIFACT_KIND,
        "preflight",
    )
    boundary = required_json_artifact(
        artifacts,
        manifest_path,
        WITHDRAWAL_BOUNDARY_ARTIFACT_KIND,
        "preflight",
    )
    minimum_withdrawal_id = validate_withdrawal_boundary(
        boundary,
        binding,
        preflight_observed_at,
        urls,
    )
    if deployment_instance_hex(details["minimum_withdrawal_id"], f"{context}.minimum_withdrawal_id") != minimum_withdrawal_id:
        fail("preflight summary differs from the withdrawal admission boundary")
    ledger_metadata = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_LEDGER_METADATA_ARTIFACT_KIND,
        "preflight",
    )
    exact_keys(
        ledger_metadata,
        {
            "schema_version",
            "kind",
            "ledger_canister_id",
            "index_canister_id",
            "index_ledger_id",
            "symbol",
            "decimals",
            "fee",
        },
        "live Ledger metadata",
    )
    if (
        ledger_metadata["schema_version"] != 1
        or ledger_metadata["kind"] != LIVE_LEDGER_METADATA_ARTIFACT_KIND
        or ledger_metadata["ledger_canister_id"] != binding["ledger_canister_id"]
        or ledger_metadata["index_canister_id"] != binding["index_canister_id"]
        or ledger_metadata["index_ledger_id"] != binding["ledger_canister_id"]
        or ledger_metadata["symbol"] != details["ledger_symbol"]
        or ledger_metadata["decimals"] != details["ledger_decimals"]
        or ledger_metadata["fee"] != details["ledger_fee"]
    ):
        fail("live Ledger metadata differs from the reviewed staging binding")
    expected_check = upgrade_instance_check(
        binding["deployment_instance_id"],
        binding["rpc_provider_urls_sha256"],
        live_public_config,
        live_canister_status,
        binding["bridge_canister_id"],
    )
    if normalized_upgrade_check(recorded_check) != expected_check:
        fail("upgrade instance check does not match the live RuntimeBinding and reviewed binding")
    summary_check = normalized_upgrade_check(
        {
            "replacement_mode": details["replacement_mode"],
            "live_schema_version": details["live_schema_version"],
            "previous_deployment_instance_id": details[
                "previous_deployment_instance_id"
            ],
            "live_module_hash": expected_check["live_module_hash"],
            "next": binding["deployment_instance_id"],
        }
    )
    if summary_check != expected_check:
        fail("preflight summary does not match the verified upgrade instance check")
    validate_upgrade_snapshots(artifacts, manifest_path)
    if (
        live_canister_status["controller_principals"]
        != details["controller_principals"]
        or require_nat(live_canister_status, "cycles_balance", "live canister status")
        != require_nat(details, "cycles_balance", context)
    ):
        fail("preflight summary differs from the live canister status snapshot")


def validate_current_schema_upgrade(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "current_schema_upgrade.details"
    expected_fields = {
        "install_mode",
        "module_sha256",
        "candid_sha256",
        "cycles_balance_before",
        "cycles_balance_after",
        "controller_principals_before",
        "controller_principals_after",
        "state_counts_before",
        "state_counts_after",
        "bridge_canister_id_before",
        "bridge_canister_id_after",
        "schema_version_before",
        "schema_version_after",
        "record_wire_version_before",
        "record_wire_version_after",
        "deployment_instance_id_before",
        "deployment_instance_id_after",
        "minimum_withdrawal_id_before",
        "minimum_withdrawal_id_after",
        "storage_integrity_after",
    }
    install_mode = require_string(details, "install_mode", context)
    if install_mode != "upgrade":
        fail("staging permits only a current-schema same-instance upgrade")
    exact_keys(details, expected_fields, context)
    if details["module_sha256"] != binding["bridge_wasm_sha256"]:
        fail("upgraded Bridge module does not match local promotion evidence")
    if details["candid_sha256"] != binding["bridge_candid_sha256"]:
        fail("upgraded Bridge Candid does not match local promotion evidence")
    cycles_before = require_nat(details, "cycles_balance_before", context)
    cycles_after = require_nat(details, "cycles_balance_after", context)
    if cycles_after <= 0 or cycles_after > cycles_before:
        fail("upgrade cycles balance must remain positive and may only decrease")
    controllers_before = details["controller_principals_before"]
    controllers_after = details["controller_principals_after"]
    if (
        not isinstance(controllers_before, list)
        or not controllers_before
        or controllers_before != controllers_after
    ):
        fail("upgrade must preserve the explicit controller set")
    before = details["state_counts_before"]
    after = details["state_counts_after"]
    if not isinstance(before, dict) or not isinstance(after, dict):
        fail("upgrade state counts must be objects")
    exact_keys(before, UPGRADE_STATE_COUNT_FIELDS, "upgrade state_counts_before")
    exact_keys(after, UPGRADE_STATE_COUNT_FIELDS, "upgrade state_counts_after")
    for field in UPGRADE_STATE_COUNT_FIELDS:
        require_nat(before, field, "upgrade state_counts_before")
        require_nat(after, field, "upgrade state_counts_after")
    if before != after:
        fail("upgrade changed persisted Bridge state counts")
    for field in ("bridge_canister_id_before", "bridge_canister_id_after"):
        if details[field] != binding["bridge_canister_id"]:
            fail("upgrade changed the Bridge Canister ID")
    for field in ("schema_version_before", "schema_version_after"):
        if require_nat(details, field, context) != CURRENT_STABLE_SCHEMA:
            fail("upgrade must start and finish on the current stable schema")
    for field in ("record_wire_version_before", "record_wire_version_after"):
        if require_nat(details, field, context) != CURRENT_RECORD_WIRE_VERSION:
            fail("upgrade must start and finish on the current record wire version")
    for field in ("deployment_instance_id_before", "deployment_instance_id_after"):
        if deployment_instance_hex(details[field], f"{context}.{field}") != binding["deployment_instance_id"]:
            fail("upgrade changed the deployment instance ID")
    for field in ("minimum_withdrawal_id_before", "minimum_withdrawal_id_after"):
        if require_deployment_instance_id(details, field, context) != binding["minimum_withdrawal_id"]:
            fail("upgrade changed the minimum withdrawal ID")
    if details["storage_integrity_after"] != "ok":
        fail("upgrade storage integrity check did not return ok")


def validate_post_upgrade_canister(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "post_upgrade_binding.canister"
    exact_keys(
        details,
        {
            "schema_version",
            "record_wire_version",
            "deployment_instance_id",
            "minimum_withdrawal_id",
            "chain_id",
            "ledger_canister_id",
            "index_canister_id",
            "evm_rpc_canister_id",
            "rpc_provider_urls_sha256",
            "expected_bridge_signer",
            "bridge_address",
            "timelock_address",
            "expected_bridge_runtime_sha256",
            "module_sha256",
            "candid_sha256",
            "governance_operator",
            "canister_deposits_paused",
            "storage_integrity",
        },
        context,
    )
    if require_nat(details, "schema_version", context) != CURRENT_STABLE_SCHEMA:
        fail(f"staging must retain current stable schema v{CURRENT_STABLE_SCHEMA}")
    if require_nat(details, "record_wire_version", context) != CURRENT_RECORD_WIRE_VERSION:
        fail(f"staging must retain current record wire v{CURRENT_RECORD_WIRE_VERSION}")
    if require_deployment_instance_id(details, "deployment_instance_id", context) != binding["deployment_instance_id"]:
        fail("post-upgrade deployment instance ID differs from the reviewed binding")
    if require_deployment_instance_id(details, "minimum_withdrawal_id", context) != binding["minimum_withdrawal_id"]:
        fail("post-upgrade minimum withdrawal ID differs from the reviewed binding")
    if details["chain_id"] != binding["chain_id"]:
        fail("post-upgrade Bridge has the wrong chain ID")
    for field in ("bridge_address", "timelock_address", "expected_bridge_signer"):
        require_pattern(details, field, EVM_ADDRESS, context)
        if details[field] != binding[field]:
            fail(f"post-upgrade {field} differs from the reviewed binding")
    require_pattern(details, "expected_bridge_runtime_sha256", EVM_HASH, context)
    if details["expected_bridge_runtime_sha256"] != binding["bridge_runtime_sha256"]:
        fail("post-upgrade Bridge runtime differs from the reviewed binding")
    if details["module_sha256"] != binding["bridge_wasm_sha256"]:
        fail("post-upgrade Bridge module differs from local promotion evidence")
    if details["candid_sha256"] != binding["bridge_candid_sha256"]:
        fail("post-upgrade Candid differs from local promotion evidence")
    for field in ("ledger_canister_id", "index_canister_id"):
        if details[field] != binding[field]:
            fail(f"post-upgrade {field} differs from the reviewed binding")
    if details["evm_rpc_canister_id"] != EVM_RPC_CANISTER_ID:
        fail("post-upgrade Bridge does not use the official EVM RPC Canister")
    if digest_hex(
        details["rpc_provider_urls_sha256"],
        f"{context}.rpc_provider_urls_sha256",
    ) != binding["rpc_provider_urls_sha256"]:
        fail("post-upgrade RPC provider digest differs from the reviewed binding")
    require_pattern(details, "governance_operator", EVM_ADDRESS, context)
    require_bool(details, "canister_deposits_paused", False, context)
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
            "bridge_runtime_template_sha256",
            "bsns_runtime_template_sha256",
            "bridge_runtime_sha256",
            "bsns_runtime_sha256",
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
    for field in ("bridge_address", "bsns_address", "timelock_address"):
        if details[field] != binding[field]:
            fail(f"staging {field} differs from the reviewed binding")
    if details["mint_signer"] != binding["expected_bridge_signer"]:
        fail("staging mint signer differs from the reviewed binding")
    for field in ("bridge_runtime_sha256", "bsns_runtime_sha256"):
        require_pattern(details, field, EVM_HASH, context)
        if details[field] != binding[field]:
            fail(f"staging {field} differs from the reviewed binding")
    if (
        details["bridge_runtime_template_sha256"] != binding["bridge_runtime_template_sha256"]
        or details["bsns_runtime_template_sha256"] != binding["bsns_runtime_template_sha256"]
    ):
        fail("staging contract runtime template differs from local promotion evidence")
    require_nat(details, "deployment_block", context)
    transactions = details["deployment_transaction_hashes"]
    if not isinstance(transactions, list) or not transactions or any(not isinstance(item, str) or not EVM_HASH.fullmatch(item) for item in transactions):
        fail("contracts must bind every deployment transaction")
    require_bool(details, "deployer_roles_zero", True, context)


def validate_post_upgrade_binding(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "post_upgrade_binding.details"
    exact_keys(details, {"canister", "contracts"}, context)
    canister = details["canister"]
    contracts = details["contracts"]
    if not isinstance(canister, dict) or not isinstance(contracts, dict):
        fail("post-upgrade Canister and contract bindings must be objects")
    validate_post_upgrade_canister(canister, binding)
    validate_contracts(contracts, binding)


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
    if require_nat(details, "delay_seconds", context) != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("activation execute did not observe the exact five-minute staging delay")
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


def validate_smoke_e2e(details: dict[str, Any]) -> None:
    context = "smoke_e2e.details"
    exact_keys(
        details,
        {
            "ic_wallet",
            "evm_wallet",
            "deposit_id",
            "deposit_transaction_hash",
            "withdrawal_id",
            "withdrawal_transaction_hash",
            "reload_state_matched",
            "base_deposits_paused",
            "base_withdrawals_paused",
            "canister_deposits_paused",
            "pending_timelock_operations",
        },
        context,
    )
    if details["ic_wallet"] != "OISY" or details["evm_wallet"] != "MetaMask":
        fail("short-delay smoke must use the reviewed OISY and MetaMask wallet pair")
    require_pattern(details, "deposit_id", EVM_HASH, context)
    require_pattern(details, "deposit_transaction_hash", EVM_HASH, context)
    require_nat(details, "withdrawal_id", context)
    require_pattern(details, "withdrawal_transaction_hash", EVM_HASH, context)
    require_bool(details, "reload_state_matched", True, context)
    for field in ("base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused"):
        require_bool(details, field, False, context)
    if require_nat(details, "pending_timelock_operations", context) != 0:
        fail("short-delay smoke left a pending Timelock operation")


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


def verify_rpc_rehearsal_manifest(path: Path) -> None:
    verifier = Path(__file__).resolve().parents[1] / "evm-rpc-rehearsal" / "rehearsal.py"
    result = subprocess.run(
        ["python3", str(verifier), "verify", str(path)],
        cwd=verifier.parents[2],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if result.returncode != 0:
        output = (result.stderr or result.stdout).strip()
        fail(f"RPC rehearsal manifest verification failed: {output}")


def validate_rpc(
    details: dict[str, Any],
    binding: dict[str, Any],
    artifacts: Any,
    manifest_path: Path,
) -> None:
    context = "rpc_rehearsal.details"
    exact_keys(
        details,
        {
            "manifest_sha256",
            "state",
            "launch_ready",
            "extended_complete",
            "scenarios",
            "providers_restored",
        },
        context,
    )
    require_pattern(details, "manifest_sha256", SHA256, context)
    if details["state"] != "EXTENDED_COMPLETE":
        fail("non-blocking detailed RPC rehearsal is not EXTENDED_COMPLETE")
    require_bool(details, "launch_ready", True, context)
    require_bool(details, "extended_complete", True, context)
    scenarios = details["scenarios"]
    if not isinstance(scenarios, list) or set(scenarios) != RPC_SCENARIOS or len(scenarios) != len(RPC_SCENARIOS):
        fail("RPC rehearsal does not bind the complete ten-scenario set")
    require_bool(details, "providers_restored", True, context)
    descriptor, rehearsal_path = required_artifact_path(
        artifacts,
        manifest_path,
        RPC_REHEARSAL_MANIFEST_ARTIFACT_KIND,
        "rpc_rehearsal",
    )
    if descriptor["sha256"] != details["manifest_sha256"]:
        fail("RPC rehearsal summary digest differs from the verified manifest")
    verify_rpc_rehearsal_manifest(rehearsal_path)
    rehearsal = load_object(rehearsal_path)
    if (
        rehearsal.get("state") != details["state"]
        or rehearsal.get("launch_ready") is not details["launch_ready"]
        or rehearsal.get("extended_complete") is not details["extended_complete"]
    ):
        fail("RPC rehearsal summary differs from the verified manifest state")
    rehearsal_scenarios = rehearsal.get("scenarios")
    if (
        not isinstance(rehearsal_scenarios, dict)
        or set(rehearsal_scenarios) != RPC_SCENARIOS
        or any(value is None for value in rehearsal_scenarios.values())
    ):
        fail("verified RPC rehearsal manifest does not contain all ten scenarios")
    rehearsal_binding = rehearsal.get("binding")
    if not isinstance(rehearsal_binding, dict):
        fail("verified RPC rehearsal manifest has no binding")
    expected_binding = {
        "base_chain_id": binding["chain_id"],
        "evm_rpc_canister_id": EVM_RPC_CANISTER_ID,
        "bridge_canister_id": binding["bridge_canister_id"],
        "ledger_canister_id": binding["ledger_canister_id"],
        "index_canister_id": binding["index_canister_id"],
        "bridge_contract": binding["bridge_address"],
        "expected_bridge_signer": binding["expected_bridge_signer"],
        "bridge_canister_wasm_sha256": binding["bridge_wasm_sha256"],
        "bridge_runtime_bytecode_sha256": binding["bridge_runtime_sha256"][2:],
    }
    for field, expected in expected_binding.items():
        if rehearsal_binding.get(field) != expected:
            fail(f"verified RPC rehearsal {field} differs from the staging binding")


def validate_rpc_provider_cross_binding(
    preflight: dict[str, Any],
    rpc_rehearsal: dict[str, Any],
    manifest_path: Path,
) -> None:
    _, rehearsal_path = required_artifact_path(
        rpc_rehearsal["artifacts"],
        manifest_path,
        RPC_REHEARSAL_MANIFEST_ARTIFACT_KIND,
        "rpc_rehearsal",
    )
    rehearsal = load_object(rehearsal_path)
    rehearsal_binding = rehearsal.get("binding")
    endpoints = (
        rehearsal_binding.get("rpc_endpoints")
        if isinstance(rehearsal_binding, dict)
        else None
    )
    if not isinstance(endpoints, list) or len(endpoints) != 3:
        fail("verified RPC rehearsal must bind the three reviewed RPC endpoints")
    digests = []
    for index, endpoint in enumerate(endpoints):
        if not isinstance(endpoint, dict):
            fail(f"verified RPC rehearsal endpoint {index} must be an object")
        digest_value = endpoint.get("url_sha256")
        if not isinstance(digest_value, str) or not SHA256.fullmatch(digest_value):
            fail(f"verified RPC rehearsal endpoint {index} has an invalid URL digest")
        digests.append(digest_value)
    if digests != preflight["details"]["configured_rpc_url_sha256"]:
        fail("verified RPC rehearsal provider order differs from the preflight binding")


def validate_refund_rehearsal(details: dict[str, Any]) -> None:
    context = "refund_rehearsal.details"
    exact_keys(
        details,
        {
            "deposit_id",
            "authorization_digest",
            "deadline",
            "at_deadline_result",
            "after_deadline_timestamp",
            "deposit_processed",
            "refund_ledger_block",
            "final_state",
            "finalized_block_number",
            "finalized_block_hash",
        },
        context,
    )
    require_pattern(details, "deposit_id", EVM_HASH, context)
    require_pattern(details, "authorization_digest", EVM_HASH, context)
    deadline = require_nat(details, "deadline", context)
    if details["at_deadline_result"] != "NotClaimable":
        fail("refund rehearsal must reject finalized time equal to the deadline")
    if require_nat(details, "after_deadline_timestamp", context) <= deadline:
        fail("refund rehearsal must use finalized time strictly after the deadline")
    require_bool(details, "deposit_processed", False, context)
    require_nat(details, "refund_ledger_block", context)
    if details["final_state"] != "Refunded":
        fail("refund rehearsal did not reach the terminal Refunded state")
    require_nat(details, "finalized_block_number", context)
    require_pattern(details, "finalized_block_hash", EVM_HASH, context)


def validate_live_acceptance(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "live_acceptance.details"
    exact_keys(
        details,
        {
            "initial_activation_operation_id",
            "reactivation_operation_id",
            "reactivation_schedule_transaction_hash",
            "reactivation_execute_transaction_hash",
            "reactivation_delay_seconds",
            "base_deposits_paused",
            "base_withdrawals_paused",
            "canister_deposits_paused",
            "pending_governance_operations",
            "pending_timelock_operations",
            "pending_deposits",
            "pending_withdrawals",
            "pending_reconciliation_jobs",
            "pending_ledger_operations",
            "reserved_deposit_mint_operations",
            "reserved_deposit_mint_amount",
            "unpaid_withdrawal_count",
            "unpaid_withdrawal_amount_out",
            "providers_restored",
            "settlement_scheduler_healthy",
            "storage_integrity",
            "reserve_sufficient",
            "schema_version",
            "record_wire_version",
            "deployment_instance_id",
            "minimum_withdrawal_id",
            "bridge_canister_id",
            "module_sha256",
            "candid_sha256",
            "frontend_profile_sha256",
            "bridge_runtime_sha256",
            "mint_authorization_ttl_seconds",
            "solidity_max_authorization_horizon_seconds",
            "old_stack_excluded",
            "monitoring_receipt_sha256",
            "finalized_block_number",
            "finalized_block_hash",
        },
        context,
    )
    initial_operation = require_pattern(
        details, "initial_activation_operation_id", EVM_HASH, context
    )
    reactivation_operation = require_pattern(
        details, "reactivation_operation_id", EVM_HASH, context
    )
    if initial_operation == reactivation_operation:
        fail("reactivation must use a new Timelock operation ID")
    require_pattern(details, "reactivation_schedule_transaction_hash", EVM_HASH, context)
    require_pattern(details, "reactivation_execute_transaction_hash", EVM_HASH, context)
    if require_nat(details, "reactivation_delay_seconds", context) != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("reactivation did not observe the exact five-minute staging delay")
    for field in ("base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused"):
        require_bool(details, field, False, context)
    for field in LIVE_ZERO_COUNT_FIELDS:
        if require_nat(details, field, context) != 0:
            fail(f"live acceptance requires zero {field}")
    for field in (
        "providers_restored",
        "settlement_scheduler_healthy",
        "reserve_sufficient",
        "old_stack_excluded",
    ):
        require_bool(details, field, True, context)
    if details["storage_integrity"] != "ok":
        fail("live acceptance requires a successful storage integrity check")
    if require_nat(details, "schema_version", context) != CURRENT_STABLE_SCHEMA:
        fail("live acceptance has the wrong stable schema")
    if require_nat(details, "record_wire_version", context) != CURRENT_RECORD_WIRE_VERSION:
        fail("live acceptance has the wrong record wire version")
    if require_deployment_instance_id(details, "deployment_instance_id", context) != binding["deployment_instance_id"]:
        fail("live acceptance changed the deployment instance ID")
    if require_deployment_instance_id(details, "minimum_withdrawal_id", context) != binding["minimum_withdrawal_id"]:
        fail("live acceptance changed the minimum withdrawal ID")
    if details["bridge_canister_id"] != binding["bridge_canister_id"]:
        fail("live acceptance changed the Bridge Canister")
    for field, binding_field in (
        ("module_sha256", "bridge_wasm_sha256"),
        ("candid_sha256", "bridge_candid_sha256"),
        ("frontend_profile_sha256", "frontend_profile_sha256"),
    ):
        if require_pattern(details, field, SHA256, context) != binding[binding_field]:
            fail(f"live acceptance {field} differs from the reviewed binding")
    if require_pattern(details, "bridge_runtime_sha256", EVM_HASH, context) != binding["bridge_runtime_sha256"]:
        fail("live acceptance Bridge runtime differs from the reviewed binding")
    if require_nat(details, "mint_authorization_ttl_seconds", context) != 600:
        fail("live acceptance requires the 600-second authorization TTL")
    if require_nat(details, "solidity_max_authorization_horizon_seconds", context) != 900:
        fail("live acceptance requires the 900-second Solidity horizon")
    require_pattern(details, "monitoring_receipt_sha256", SHA256, context)
    require_nat(details, "finalized_block_number", context)
    require_pattern(details, "finalized_block_hash", EVM_HASH, context)


def validate_reactivation_receipts(
    details: dict[str, Any],
    binding: dict[str, Any],
    artifacts: Any,
    manifest_path: Path,
) -> None:
    schedule = required_json_artifact(
        artifacts,
        manifest_path,
        REACTIVATION_SCHEDULE_RECEIPT_ARTIFACT_KIND,
        "live_acceptance",
    )
    schedule_context = "reactivation schedule receipt"
    exact_keys(
        schedule,
        {
            "schema_version",
            "kind",
            "chain_id",
            "timelock_address",
            "operation_id",
            "transaction_hash",
            "success",
            "block_number",
            "block_hash",
            "block_timestamp",
            "delay_seconds",
            "ready_timestamp",
        },
        schedule_context,
    )
    if (
        schedule["schema_version"] != 1
        or schedule["kind"] != REACTIVATION_SCHEDULE_RECEIPT_ARTIFACT_KIND
    ):
        fail(f"{schedule_context} has the wrong schema or kind")
    if (
        require_nat(schedule, "chain_id", schedule_context) != binding["chain_id"]
        or schedule.get("timelock_address") != binding["timelock_address"]
        or require_pattern(schedule, "operation_id", EVM_HASH, schedule_context)
        != details["reactivation_operation_id"]
        or require_pattern(schedule, "transaction_hash", EVM_HASH, schedule_context)
        != details["reactivation_schedule_transaction_hash"]
    ):
        fail(f"{schedule_context} differs from the staging activation binding")
    require_bool(schedule, "success", True, schedule_context)
    require_nat(schedule, "block_number", schedule_context)
    require_pattern(schedule, "block_hash", EVM_HASH, schedule_context)
    block_timestamp = require_nat(schedule, "block_timestamp", schedule_context)
    delay = require_nat(schedule, "delay_seconds", schedule_context)
    ready_timestamp = require_nat(schedule, "ready_timestamp", schedule_context)
    if delay != ACTIVATION_TIMELOCK_DELAY_SECONDS or ready_timestamp != block_timestamp + delay:
        fail(f"{schedule_context} does not prove the exact five-minute delay")

    execute = required_json_artifact(
        artifacts,
        manifest_path,
        REACTIVATION_EXECUTE_RECEIPT_ARTIFACT_KIND,
        "live_acceptance",
    )
    execute_context = "reactivation execute receipt"
    exact_keys(
        execute,
        {
            "schema_version",
            "kind",
            "chain_id",
            "timelock_address",
            "operation_id",
            "transaction_hash",
            "success",
            "block_number",
            "block_hash",
            "block_timestamp",
        },
        execute_context,
    )
    if (
        execute["schema_version"] != 1
        or execute["kind"] != REACTIVATION_EXECUTE_RECEIPT_ARTIFACT_KIND
    ):
        fail(f"{execute_context} has the wrong schema or kind")
    if (
        require_nat(execute, "chain_id", execute_context) != binding["chain_id"]
        or execute.get("timelock_address") != binding["timelock_address"]
        or require_pattern(execute, "operation_id", EVM_HASH, execute_context)
        != details["reactivation_operation_id"]
        or require_pattern(execute, "transaction_hash", EVM_HASH, execute_context)
        != details["reactivation_execute_transaction_hash"]
    ):
        fail(f"{execute_context} differs from the staging activation binding")
    require_bool(execute, "success", True, execute_context)
    execute_block = require_nat(execute, "block_number", execute_context)
    require_pattern(execute, "block_hash", EVM_HASH, execute_context)
    execute_timestamp = require_nat(execute, "block_timestamp", execute_context)
    if execute_block < schedule["block_number"] or execute_timestamp < ready_timestamp:
        fail(f"{execute_context} predates the proven Timelock ready point")


def validate_monitoring_receipt(
    details: dict[str, Any],
    artifacts: Any,
    manifest_path: Path,
) -> None:
    descriptor, target = required_artifact_path(
        artifacts,
        manifest_path,
        STAGING_MONITORING_RECEIPT_ARTIFACT_KIND,
        "live_acceptance",
    )
    if descriptor["sha256"] != details["monitoring_receipt_sha256"]:
        fail("live acceptance monitoring digest differs from the saved receipt")
    receipt = load_object(target)
    context = "staging monitoring receipt"
    exact_keys(
        receipt,
        {"artifact_schema_version", "kind", "observed_at", *LIVE_MONITOR_FIELDS},
        context,
    )
    if (
        receipt["artifact_schema_version"] != 1
        or receipt["kind"] != STAGING_MONITORING_RECEIPT_ARTIFACT_KIND
    ):
        fail(f"{context} has the wrong schema or kind")
    validate_timestamp(require_string(receipt, "observed_at", context), f"{context}.observed_at")
    for field in LIVE_MONITOR_FIELDS:
        if receipt[field] != details[field]:
            fail(f"{context}.{field} differs from live acceptance")


VALIDATORS = {
    "bootstrap_attestation": validate_bootstrap_attestation,
    "preflight": validate_preflight,
    "current_schema_upgrade": validate_current_schema_upgrade,
    "post_upgrade_binding": validate_post_upgrade_binding,
    "frontend_publish": validate_frontend,
    "smoke_e2e": validate_smoke_e2e,
    "wallet_e2e": validate_wallet_e2e,
    "refund_rehearsal": validate_refund_rehearsal,
    "rpc_rehearsal": validate_rpc,
    "live_acceptance": validate_live_acceptance,
}


def validate_binding(binding: Any) -> dict[str, Any]:
    if not isinstance(binding, dict):
        fail("binding must be an object")
    expected = {
        "source_commit",
        "local_e2e_sha256",
        "bridge_wasm_sha256",
        "bridge_candid_sha256",
        "bridge_runtime_template_sha256",
        "bsns_runtime_template_sha256",
        "frontend_profile_sha256",
        "bridge_canister_id",
        "ledger_canister_id",
        "index_canister_id",
        "environment_mode",
        "activation_timelock_delay_seconds",
        "stable_schema_version",
        "record_wire_version",
        "deployment_instance_id",
        "minimum_withdrawal_id",
        "chain_id",
        "bridge_address",
        "bsns_address",
        "timelock_address",
        "bridge_runtime_sha256",
        "bsns_runtime_sha256",
        "expected_bridge_signer",
        "rpc_provider_urls_sha256",
    }
    exact_keys(binding, expected, "binding")
    require_pattern(binding, "source_commit", GIT_COMMIT, "binding")
    for field in (
        "local_e2e_sha256",
        "bridge_wasm_sha256",
        "bridge_candid_sha256",
        "frontend_profile_sha256",
    ):
        require_pattern(binding, field, SHA256, "binding")
    for field in ("bridge_runtime_template_sha256", "bsns_runtime_template_sha256"):
        require_pattern(binding, field, EVM_HASH, "binding")
    require_deployment_instance_id(binding, "deployment_instance_id", "binding")
    require_deployment_instance_id(binding, "minimum_withdrawal_id", "binding")
    if require_nat(binding, "chain_id", "binding") != CHAIN_ID:
        fail("binding has the wrong chain ID")
    for field in ("bridge_address", "bsns_address", "timelock_address", "expected_bridge_signer"):
        require_pattern(binding, field, EVM_ADDRESS, "binding")
    for field in ("bridge_runtime_sha256", "bsns_runtime_sha256"):
        require_pattern(binding, field, EVM_HASH, "binding")
    require_pattern(binding, "rpc_provider_urls_sha256", EVM_HASH, "binding")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        require_pattern(binding, field, PRINCIPAL, "binding")
    if binding["environment_mode"] != ENVIRONMENT_MODE:
        fail("binding is not the short-delay test-only environment")
    if require_nat(binding, "activation_timelock_delay_seconds", "binding") != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("binding has the wrong staging activation delay")
    if (
        require_nat(binding, "stable_schema_version", "binding")
        != CURRENT_STABLE_SCHEMA
        or require_nat(binding, "record_wire_version", "binding")
        != CURRENT_RECORD_WIRE_VERSION
    ):
        fail("binding has the wrong stable schema or record wire version")
    return binding


def validate_stage(stage: str, evidence: Any, manifest_path: Path, binding: dict[str, Any], verify_files: bool) -> None:
    if not isinstance(evidence, dict):
        fail(f"{stage} evidence must be an object")
    exact_keys(evidence, {"schema_version", "stage", "observed_at", "source_commit", "artifacts", "details"}, f"{stage} evidence")
    if evidence["schema_version"] != SCHEMA_VERSION or evidence["stage"] != stage:
        fail(f"{stage} evidence has the wrong schema or stage")
    if evidence["source_commit"] != binding["source_commit"]:
        fail(f"{stage} evidence is not bound to the reviewed source commit")
    stage_observed_at = require_string(evidence, "observed_at", f"{stage} evidence")
    validate_timestamp(stage_observed_at, f"{stage}.observed_at")
    validate_artifacts(evidence["artifacts"], manifest_path, stage, verify_files)
    details = evidence["details"]
    if not isinstance(details, dict):
        fail(f"{stage}.details must be an object")
    validator = VALIDATORS[stage]
    if stage == "bootstrap_attestation":
        validator(details, binding, evidence["artifacts"], manifest_path)
    elif stage == "preflight":
        validator(
            details,
            binding,
            evidence["artifacts"],
            manifest_path,
            stage_observed_at,
        )
    elif stage == "rpc_rehearsal":
        validator(details, binding, evidence["artifacts"], manifest_path)
    elif stage == "live_acceptance":
        validator(details, binding)
        validate_reactivation_receipts(
            details, binding, evidence["artifacts"], manifest_path
        )
        validate_monitoring_receipt(details, evidence["artifacts"], manifest_path)
    elif stage in {
        "current_schema_upgrade",
        "post_upgrade_binding",
        "frontend_publish",
    }:
        validator(details, binding)
    else:
        validator(details)
    if stage in STAGE_RECEIPT_KINDS:
        validate_stage_receipt(stage, evidence, manifest_path)


def validate_upgrade_cross_binding(
    preflight: dict[str, Any],
    upgrade: dict[str, Any],
    manifest_path: Path,
) -> None:
    details = upgrade["details"]
    artifacts = preflight["artifacts"]
    bridge_status = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_BRIDGE_STATUS_ARTIFACT_KIND,
        "preflight",
    )
    if details["state_counts_before"] != bridge_status:
        fail("upgrade before-state counts differ from the preflight snapshot")
    canister_status = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_CANISTER_STATUS_ARTIFACT_KIND,
        "preflight",
    )
    if (
        details["controller_principals_before"]
        != canister_status.get("controller_principals")
        or details["cycles_balance_before"] != canister_status.get("cycles_balance")
    ):
        fail("upgrade before-state differs from the preflight Canister status")
    public_config = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_PUBLIC_CONFIG_ARTIFACT_KIND,
        "preflight",
    )
    if (
        details["schema_version_before"] != public_config.get("schema_version")
        or deployment_instance_hex(
            public_config.get("deployment_instance_id"),
            "preflight deployment_instance_id",
        )
        != deployment_instance_hex(
            details["deployment_instance_id_before"],
            "upgrade deployment_instance_id_before",
        )
    ):
        fail("upgrade identity differs from the preflight RuntimeBinding")
    boundary = required_json_artifact(
        artifacts,
        manifest_path,
        WITHDRAWAL_BOUNDARY_ARTIFACT_KIND,
        "preflight",
    )
    if details["minimum_withdrawal_id_before"] != boundary.get("minimum_withdrawal_id"):
        fail("upgrade minimum withdrawal ID differs from the preflight boundary")


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
    preflight = stages["preflight"]
    upgrade = stages["current_schema_upgrade"]
    if preflight is not None and upgrade is not None:
        if (
            preflight["details"]["replacement_mode"] != CURRENT_SCHEMA_UPGRADE
            or upgrade["details"]["install_mode"] != "upgrade"
        ):
            fail("upgrade mode does not match the preflight replacement mode")
        validate_upgrade_cross_binding(preflight, upgrade, path)
    rpc_rehearsal = stages["rpc_rehearsal"]
    if preflight is not None and rpc_rehearsal is not None:
        validate_rpc_provider_cross_binding(preflight, rpc_rehearsal, path)
    expected_state = manifest_state(completed)
    expected_complete = len(completed) == len(STAGES)
    if manifest["state"] != expected_state or manifest["complete"] is not expected_complete:
        fail("manifest state does not match its recorded stages")
    if require_complete and not expected_complete:
        fail(f"staging E2E is incomplete: {manifest['state']}")


def manifest_state(completed: list[str]) -> str:
    if len(completed) == len(STAGES):
        return "SHORT_DELAY_LIVE"
    return f"AWAITING_{STAGES[len(completed)].upper()}"


def validate_local_upgrade_state(value: Any, context: str) -> str:
    if not isinstance(value, dict):
        fail(f"{context} must be an object")
    required = {
        "owner_sequence",
        "status",
        "runtime_binding",
        "operational_config",
        "deposits",
        "withdrawals",
        "audit_events",
        "activation_status",
        "storage_integrity",
    }
    if not required.issubset(value):
        fail(f"{context} is missing required upgrade state")
    if not isinstance(value["owner_sequence"], str) or not value["owner_sequence"].isdigit():
        fail(f"{context}.owner_sequence must be a decimal string")
    status = value["status"]
    if not isinstance(status, dict) or not {
        "schema_version",
        "counts",
        "settlement_scheduler",
    }.issubset(status):
        fail(f"{context}.status is incomplete")
    try:
        schema_version = int(status["schema_version"])
    except (TypeError, ValueError):
        fail(f"{context}.status.schema_version is invalid")
    if schema_version != CURRENT_STABLE_SCHEMA:
        fail(f"{context} must use stable schema v{CURRENT_STABLE_SCHEMA}")
    counts = status["counts"]
    if not isinstance(counts, dict) or not {
        "pending_ledger_operations",
        "reserved_deposit_mint_operations",
    }.issubset(counts):
        fail(f"{context}.status.counts is incomplete")
    if not isinstance(status["settlement_scheduler"], dict):
        fail(f"{context}.status.settlement_scheduler must be an object")
    runtime = value["runtime_binding"]
    runtime_fields = {
        "deployment_instance_id",
        "minimum_withdrawal_id",
        "base_chain_id",
        "bridge_contract",
        "expected_bridge_runtime_sha256",
        "timelock_contract",
        "expected_bridge_signer",
        "ledger_canister_id",
        "index_canister_id",
        "evm_rpc_canister_id",
        "rpc_provider_urls_sha256",
        "schema_version",
        "operational_config_sha256",
    }
    if not isinstance(runtime, dict) or not runtime_fields.issubset(runtime):
        fail(f"{context}.runtime_binding is incomplete")
    operational_fields = {
        "deposit_rate_limit_window_seconds",
        "deposit_rate_limit_global",
        "deposit_rate_limit_per_principal",
        "notification_rate_limit_window_seconds",
        "notification_rate_limit_global",
        "notification_ingestion_rate_limit_global",
        "settlement_rate_limit_window_seconds",
        "settlement_rate_limit_global",
        "settlement_rate_limit_per_principal",
        "settlement_rate_limit_per_record",
        "settlement_retry_interval_seconds",
    }
    if not isinstance(value["operational_config"], dict) or not operational_fields.issubset(
        value["operational_config"]
    ):
        fail(f"{context}.operational_config is incomplete")
    if not isinstance(value["deposits"], list) or not value["deposits"]:
        fail(f"{context}.deposits must preserve at least one Deposit")
    if not isinstance(value["withdrawals"], list):
        fail(f"{context}.withdrawals must be an array")
    if not isinstance(value["audit_events"], dict):
        fail(f"{context}.audit_events must be an object")
    activation = value["activation_status"]
    if not isinstance(activation, dict) or not {
        "pending_timelock_operation",
        "deposits_paused",
    }.issubset(activation):
        fail(f"{context}.activation_status is incomplete")
    if value["storage_integrity"] != "ok":
        fail(f"{context}.storage_integrity must be ok")
    return deployment_instance_hex(
        runtime["deployment_instance_id"],
        f"{context}.runtime_binding.deployment_instance_id",
    )


def validate_local_promotion_evidence(local: dict[str, Any]) -> dict[str, Any]:
    context = "local promotion evidence"
    exact_keys(
        local,
        {
            "schema_version",
            "environment_mode",
            "activation_timelock_delay_seconds",
            "stable_schema_version",
            "record_wire_version",
            "deployment_instance_id",
            "created_at",
            "source_commit",
            "bridge_wasm_sha256",
            "bridge_runtime_template_sha256",
            "bsns_runtime_template_sha256",
            "candid_sha256",
            "bridge_abi_sha256",
            "bsns_abi_sha256",
            "ledger_release",
            "ledger_wasm_sha256",
            "index_wasm_sha256",
            "state_upgrade",
            "tests",
        },
        context,
    )
    if local["schema_version"] != SCHEMA_VERSION:
        fail("local promotion evidence is not schema v8")
    if (
        require_nat(local, "stable_schema_version", context) != CURRENT_STABLE_SCHEMA
        or require_nat(local, "record_wire_version", context)
        != CURRENT_RECORD_WIRE_VERSION
    ):
        fail("local promotion evidence has the wrong stable schema or record wire version")
    if (
        local["environment_mode"] != ENVIRONMENT_MODE
        or local["activation_timelock_delay_seconds"]
        != ACTIVATION_TIMELOCK_DELAY_SECONDS
    ):
        fail("local promotion evidence is not bound to the five-minute staging policy")
    validate_timestamp(require_string(local, "created_at", context), f"{context}.created_at")
    require_pattern(local, "source_commit", GIT_COMMIT, context)
    for field in (
        "bridge_wasm_sha256",
        "candid_sha256",
        "bridge_abi_sha256",
        "bsns_abi_sha256",
    ):
        require_pattern(local, field, SHA256, context)
    for field in ("bridge_runtime_template_sha256", "bsns_runtime_template_sha256"):
        require_pattern(local, field, EVM_HASH, context)
    if local["ledger_release"] != "ledger-suite-icrc-2026-03-09":
        fail("local promotion evidence has the wrong Ledger release")
    if local["ledger_wasm_sha256"] != "354dd6ecfdc72b5409805b31dea22c9db11df6e14095a5a68924eb63535e6d8a":
        fail("local promotion evidence has the wrong Ledger Wasm")
    if local["index_wasm_sha256"] != "dab6808d0dfc06e5e88336d0c3d3e45e5448c6e36c2a781f3e9e09bd450f528c":
        fail("local promotion evidence has the wrong Index Wasm")
    required_tests = {
        "full_local_ci",
        "real_frontend_e2e",
        "canister_activation",
        "timelock_delay_enforced",
        "state_upgrade",
    }
    tests = local["tests"]
    if (
        not isinstance(tests, dict)
        or set(tests) != required_tests
        or any(tests[name] != "passed" for name in required_tests)
    ):
        fail("local promotion evidence is not a complete schema v8 pass")
    upgrade = local["state_upgrade"]
    if not isinstance(upgrade, dict):
        fail("local promotion evidence has no verified same-Wasm upgrade")
    exact_keys(upgrade, {"verified", "before", "after"}, f"{context}.state_upgrade")
    if upgrade["verified"] is not True or upgrade["before"] != upgrade["after"]:
        fail("local promotion evidence has no exact same-Wasm state preservation")
    before_instance = validate_local_upgrade_state(
        upgrade["before"], f"{context}.state_upgrade.before"
    )
    after_instance = validate_local_upgrade_state(
        upgrade["after"], f"{context}.state_upgrade.after"
    )
    top_instance = require_deployment_instance_id(
        local, "deployment_instance_id", context
    )
    if before_instance != top_instance or after_instance != top_instance:
        fail("local promotion evidence deployment instance is inconsistent")
    return upgrade


def initialize(output: Path, local_evidence_path: Path, profile_path: Path, repo_root: Path | None = None) -> None:
    if output.exists():
        fail(f"refusing to overwrite existing manifest: {output}")
    local = load_object(local_evidence_path)
    profile = load_object(profile_path)
    validate_local_promotion_evidence(local)
    if profile.get("environment") != "sepolia-staging" or profile.get("testOnly") is not True or profile.get("chainId") != CHAIN_ID:
        fail("frontend profile is not the Base Sepolia test-only profile")
    if profile.get("evmRpcCanisterId") != EVM_RPC_CANISTER_ID:
        fail("frontend profile does not use the official EVM RPC Canister")
    if profile.get("environmentMode") != ENVIRONMENT_MODE or profile.get("activationTimelockDelaySeconds") != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("frontend profile is not bound to the five-minute staging policy")
    if repo_root is not None:
        resolved_repo_root = repo_root.resolve()
        resolved_local_evidence = local_evidence_path.resolve()
        try:
            resolved_local_evidence.relative_to(resolved_repo_root)
        except ValueError:
            pass
        else:
            fail("local promotion evidence must be generated outside the repository")
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
        if status.stdout.strip():
            fail("staging E2E initialization requires a clean working tree")
        if local.get("source_commit") != head.stdout.strip():
            fail("local promotion evidence is stale for the current source commit")
    binding = {
        "source_commit": local.get("source_commit"),
        "local_e2e_sha256": digest(local_evidence_path),
        "bridge_wasm_sha256": local.get("bridge_wasm_sha256"),
        "bridge_candid_sha256": local.get("candid_sha256"),
        "bridge_runtime_template_sha256": local.get("bridge_runtime_template_sha256"),
        "bsns_runtime_template_sha256": local.get("bsns_runtime_template_sha256"),
        "frontend_profile_sha256": digest(profile_path),
        "bridge_canister_id": profile.get("bridgeCanisterId"),
        "ledger_canister_id": profile.get("ledgerCanisterId"),
        "index_canister_id": profile.get("indexCanisterId"),
        "environment_mode": local.get("environment_mode"),
        "activation_timelock_delay_seconds": local.get("activation_timelock_delay_seconds"),
        "stable_schema_version": local.get("stable_schema_version"),
        "record_wire_version": local.get("record_wire_version"),
        "deployment_instance_id": profile.get("deploymentInstanceId"),
        "minimum_withdrawal_id": profile.get("minimumWithdrawalId"),
        "chain_id": profile.get("chainId"),
        "bridge_address": profile.get("bridgeAddress"),
        "bsns_address": profile.get("bsnsAddress"),
        "timelock_address": profile.get("timelockAddress"),
        "bridge_runtime_sha256": profile.get("bridgeRuntimeHash"),
        "bsns_runtime_sha256": profile.get("bsnsRuntimeHash"),
        "expected_bridge_signer": profile.get("expected_bridge_signer"),
        "rpc_provider_urls_sha256": profile.get("rpcProviderUrlsSha256"),
    }
    validate_binding(binding)
    timestamp = now()
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "kind": KIND,
        "state": "AWAITING_BOOTSTRAP_ATTESTATION",
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
    if stage == "current_schema_upgrade":
        preflight = manifest["stages"]["preflight"]
        if (
            preflight["details"]["replacement_mode"] != CURRENT_SCHEMA_UPGRADE
            or evidence["details"]["install_mode"] != "upgrade"
        ):
            fail("upgrade mode does not match the preflight replacement mode")
        validate_upgrade_cross_binding(preflight, evidence, manifest_path)
    if stage == "rpc_rehearsal":
        validate_rpc_provider_cross_binding(
            manifest["stages"]["preflight"], evidence, manifest_path
        )
    manifest["stages"][stage] = evidence
    manifest["updated_at"] = now()
    next_stage = next((name for name in STAGES if manifest["stages"][name] is None), None)
    manifest["complete"] = next_stage is None
    completed = [stage for stage in STAGES if manifest["stages"][stage] is not None]
    manifest["state"] = manifest_state(completed)
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
