#!/usr/bin/env python3
"""Build and verify the test-only IC mainnet × Base Sepolia E2E manifest."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 7
KIND = "kinic-bridge-sepolia-staging-e2e"
CHAIN_ID = 84532
ENVIRONMENT_MODE = "short-delay-test-only"
ACTIVATION_TIMELOCK_DELAY_SECONDS = 300
EVM_RPC_CANISTER_ID = "7hfb6-caaaa-aaaar-qadga-cai"
CURRENT_STABLE_SCHEMA = 31
OBSOLETE_REPLACEABLE_SCHEMA = 30
OBSOLETE_REPLACEMENT_POLICY_PATH = (
    Path(__file__).resolve().parents[2]
    / "deployments/sepolia-staging/obsolete-replacement-policy.json"
)
OBSOLETE_PAUSE_COLLECTOR_PATH = Path(__file__).with_name(
    "capture-obsolete-pause-evidence.mjs"
)
OBSOLETE_REPLACEMENT_POLICY = json.loads(
    OBSOLETE_REPLACEMENT_POLICY_PATH.read_text(encoding="utf-8")
)
LIVE_PUBLIC_CONFIG_ARTIFACT_KIND = "live-public-config"
REINSTALL_INSTANCE_CHECK_ARTIFACT_KIND = "reinstall-instance-check"
OBSOLETE_STATE_DISPOSITION_ARTIFACT_KIND = "obsolete-state-disposition"
OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND = "obsolete-pause-evidence"
LIVE_BRIDGE_STATUS_ARTIFACT_KIND = "live-bridge-status"
LIVE_ACTIVATION_STATUS_ARTIFACT_KIND = "live-activation-status"
LIVE_CANISTER_STATUS_ARTIFACT_KIND = "live-canister-status"
LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND = "live-storage-integrity"
LIVE_LEDGER_BALANCE_ARTIFACT_KIND = "live-ledger-balance"
CURRENT_SCHEMA_REINSTALL = "current-schema-reinstall"
OBSOLETE_SCHEMA_UPGRADE = "obsolete-schema-upgrade"
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
OBSOLETE_PAUSE_ACTIONS = {
    "PauseDepositMints": {
        "calldata_hex": "0x15415f22",
        "event_topic": "0x7a8cbbf7de1b70cf6a63059c06484e4a6ca4b28f18ced89f03ea751608fc29a1",
    },
    "PauseWithdrawals": {
        "calldata_hex": "0x56bb54a7",
        "event_topic": "0x7c82b8b6bc44325506945ff406eeb0f2add5b91cfdd2265e80994967d30a787d",
    },
}
STAGES = (
    "preflight",
    "contracts",
    "install",
    "initialize",
    "activation_schedule",
    "activation_execute",
    "frontend_publish",
    "smoke_e2e",
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


def write_new_object_atomically(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            delete=False,
        ) as temporary:
            temporary_path = Path(temporary.name)
            temporary.write(json.dumps(value, indent=2, sort_keys=True) + "\n")
            temporary.flush()
            os.fsync(temporary.fileno())
        try:
            os.link(temporary_path, path)
        except FileExistsError:
            fail(f"refusing to overwrite existing pause evidence: {path}")
    finally:
        if temporary_path is not None:
            temporary_path.unlink(missing_ok=True)


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


def required_json_artifact(
    artifacts: Any,
    manifest_path: Path,
    kind: str,
    context: str,
) -> dict[str, Any]:
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
    return load_object(target)


def reinstall_instance_check(
    next_deployment_instance_id: Any,
    live_public_config: dict[str, Any],
    live_canister_status: dict[str, Any],
    bridge_canister_id: str,
) -> dict[str, Any]:
    next_id = deployment_instance_hex(
        next_deployment_instance_id,
        "binding deployment_instance_id",
    )
    schema_version = require_nat(
        live_public_config,
        "schema_version",
        "live PublicConfig",
    )
    if schema_version not in {CURRENT_STABLE_SCHEMA, OBSOLETE_REPLACEABLE_SCHEMA}:
        fail(
            "staging install check only accepts current schema "
            f"v{CURRENT_STABLE_SCHEMA} or audited obsolete schema "
            f"v{OBSOLETE_REPLACEABLE_SCHEMA}"
        )
    previous = deployment_instance_hex(
        live_public_config.get("deployment_instance_id"),
        "live PublicConfig deployment_instance_id",
    )
    if schema_version == CURRENT_STABLE_SCHEMA and previous == next_id:
        fail("staging reinstall rejected reuse of the live deployment instance ID")
    if schema_version == OBSOLETE_REPLACEABLE_SCHEMA and previous != next_id:
        fail("staging upgrade must preserve the live deployment instance ID")
    live_module_hash = require_pattern(
        live_canister_status,
        "module_hash",
        EVM_HASH,
        "live canister status",
    )
    if live_module_hash == "0x" + "0" * 64:
        fail("live canister status module_hash must be nonzero")
    replacement_mode = (
        CURRENT_SCHEMA_REINSTALL
        if schema_version == CURRENT_STABLE_SCHEMA
        else OBSOLETE_SCHEMA_UPGRADE
    )
    if replacement_mode == OBSOLETE_SCHEMA_UPGRADE:
        policy = OBSOLETE_REPLACEMENT_POLICY
        exact_keys(
            policy,
            {
                "schema_version",
                "environment",
                "bridge_canister_id",
                "live_schema_version",
                "previous_deployment_instance_id",
                "module_hash",
            },
            "obsolete replacement policy",
        )
        if (
            policy["schema_version"] != 1
            or policy["environment"] != "sepolia-staging"
            or bridge_canister_id != policy["bridge_canister_id"]
            or schema_version != policy["live_schema_version"]
            or previous != policy["previous_deployment_instance_id"]
            or live_module_hash != policy["module_hash"]
        ):
            fail("obsolete staging upgrade does not match the reviewed replacement policy")
    return {
        "replacement_mode": replacement_mode,
        "live_schema_version": schema_version,
        "previous_deployment_instance_id": previous,
        "live_module_hash": live_module_hash,
        "next": next_id,
    }


def normalized_reinstall_check(value: dict[str, Any]) -> dict[str, Any]:
    context = "reinstall instance check"
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
    expected_mode = {
        CURRENT_STABLE_SCHEMA: CURRENT_SCHEMA_REINSTALL,
        OBSOLETE_REPLACEABLE_SCHEMA: OBSOLETE_SCHEMA_UPGRADE,
    }.get(schema_version)
    if replacement_mode != expected_mode:
        fail(f"{context} replacement_mode does not match live_schema_version")
    previous_value = value["previous_deployment_instance_id"]
    previous = (
        None
        if previous_value is None
        else deployment_instance_hex(previous_value, f"{context} previous_deployment_instance_id")
    )
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
        "next": deployment_instance_hex(value["next"], f"{context} next"),
    }


def validate_obsolete_pause_evidence(
    value: dict[str, Any],
    binding: dict[str, Any],
    expected_check: dict[str, Any],
) -> None:
    context = "obsolete pause evidence"
    exact_keys(
        value,
        {
            "schema_version",
            "environment",
            "capture_id",
            "capture_started_at",
            "observed_at",
            "bridge_canister_id",
            "chain_id",
            "bridge_address",
            "providers",
            "ic_pause",
            "ic_live",
            "complete",
        },
        context,
    )
    if value["schema_version"] != 2 or value["environment"] != "sepolia-staging":
        fail(f"{context} must be a schema v2 Sepolia staging record")
    capture_id = require_string(value, "capture_id", context)
    if not re.fullmatch(r"[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}", capture_id):
        fail(f"{context}.capture_id must be a UUIDv4")
    started_at = validate_timestamp(require_string(value, "capture_started_at", context), f"{context}.capture_started_at")
    completed_at = validate_timestamp(require_string(value, "observed_at", context), f"{context}.observed_at")
    if completed_at < started_at or (completed_at - started_at).total_seconds() > 300:
        fail(f"{context} capture must complete within five minutes")
    if (completed_at - datetime.now(timezone.utc)).total_seconds() > 60:
        fail(f"{context} capture timestamp is too far in the future")
    ic_live = value["ic_live"]
    if not isinstance(ic_live, dict):
        fail(f"{context}.ic_live must be an object")
    exact_keys(
        ic_live,
        {"observed_at", "live_schema_version", "previous_deployment_instance_id", "live_module_hash", "status_deposits_paused"},
        f"{context}.ic_live",
    )
    ic_live_at = validate_timestamp(require_string(ic_live, "observed_at", f"{context}.ic_live"), f"{context}.ic_live.observed_at")
    if ic_live_at < started_at or ic_live_at > completed_at or (completed_at - ic_live_at).total_seconds() > 60:
        fail(f"{context}.ic_live must be observed in the final capture minute")
    if (
        value["bridge_canister_id"] != binding["bridge_canister_id"]
        or require_nat(value, "chain_id", context) != CHAIN_ID
        or require_nat(ic_live, "live_schema_version", f"{context}.ic_live")
        != expected_check["live_schema_version"]
        or deployment_instance_hex(
            ic_live["previous_deployment_instance_id"],
            f"{context}.ic_live.previous_deployment_instance_id",
        )
        != expected_check["previous_deployment_instance_id"]
        or require_pattern(ic_live, "live_module_hash", EVM_HASH, f"{context}.ic_live")
        != expected_check["live_module_hash"]
        or ic_live["status_deposits_paused"] is not True
        or require_string(value, "bridge_address", context).lower()
        != str(binding["bridge_address"]).lower()
        or value["complete"] is not True
    ):
        fail(f"{context} differs from the reviewed replacement binding")

    providers = value["providers"]
    if not isinstance(providers, list) or len(providers) != 3:
        fail(f"{context} requires exactly three provider observations")
    provider_digests: set[str] = set()
    provider_observed_at: list[datetime] = []
    heads: list[tuple[int, str]] = []
    normalized_actions: list[dict[str, tuple[Any, ...]]] = []
    action_fields = {
        "kind",
        "transaction_hash",
        "block_number",
        "block_hash",
        "receipt_status",
        "target",
        "calldata_hex",
        "event_topic",
        "event_observed",
        "canonical_probe_succeeded",
    }
    for index, provider in enumerate(providers):
        provider_context = f"{context}.providers[{index}]"
        if not isinstance(provider, dict):
            fail(f"{provider_context} must be an object")
        exact_keys(
            provider,
            {
                "provider_url_sha256",
                "chain_id",
                "finalized_block_number",
                "finalized_block_hash",
                "actions",
                "observed_at",
            },
            provider_context,
        )
        provider_digest = require_pattern(
            provider,
            "provider_url_sha256",
            SHA256,
            provider_context,
        )
        provider_digests.add(provider_digest)
        provider_at = validate_timestamp(require_string(provider, "observed_at", provider_context), f"{provider_context}.observed_at")
        if provider_at < started_at or provider_at > completed_at:
            fail(f"{provider_context} timestamp is outside the capture window")
        provider_observed_at.append(provider_at)
        if require_nat(provider, "chain_id", provider_context) != CHAIN_ID:
            fail(f"{provider_context} observed the wrong chain")
        finalized_number = require_nat(
            provider,
            "finalized_block_number",
            provider_context,
        )
        finalized_hash = require_pattern(
            provider,
            "finalized_block_hash",
            EVM_HASH,
            provider_context,
        )
        heads.append((finalized_number, finalized_hash))
        actions = provider["actions"]
        if not isinstance(actions, list) or len(actions) != len(OBSOLETE_PAUSE_ACTIONS):
            fail(f"{provider_context} must contain both Base pause actions")
        normalized: dict[str, tuple[Any, ...]] = {}
        for action in actions:
            if not isinstance(action, dict):
                fail(f"{provider_context}.actions must contain objects")
            exact_keys(action, action_fields, f"{provider_context}.action")
            kind = require_string(action, "kind", f"{provider_context}.action")
            expected = OBSOLETE_PAUSE_ACTIONS.get(kind)
            if expected is None or kind in normalized:
                fail(f"{provider_context} has an unknown or duplicate pause action")
            transaction_hash = require_pattern(
                action,
                "transaction_hash",
                EVM_HASH,
                f"{provider_context}.{kind}",
            )
            block_number = require_nat(
                action,
                "block_number",
                f"{provider_context}.{kind}",
            )
            block_hash = require_pattern(
                action,
                "block_hash",
                EVM_HASH,
                f"{provider_context}.{kind}",
            )
            target = require_string(action, "target", f"{provider_context}.{kind}").lower()
            if (
                require_nat(action, "receipt_status", f"{provider_context}.{kind}") != 1
                or block_number > finalized_number
                or target != str(binding["bridge_address"]).lower()
                or action["calldata_hex"].lower() != expected["calldata_hex"]
                or action["event_topic"].lower() != expected["event_topic"]
                or action["event_observed"] is not True
                or action["canonical_probe_succeeded"] is not True
            ):
                fail(f"{provider_context}.{kind} is not a canonical successful pause")
            normalized[kind] = (
                transaction_hash,
                block_number,
                block_hash,
                target,
                action["calldata_hex"].lower(),
                action["event_topic"].lower(),
            )
        normalized_actions.append(normalized)
    if len(provider_digests) != 3:
        fail(f"{context} requires three distinct provider URL digests")
    head_winner = max(set(heads), key=heads.count)
    eligible = [index for index, head in enumerate(heads) if head == head_winner]
    if len(eligible) < 2:
        fail(f"{context} Finalized head has no 2-of-3 agreement")
    for kind in OBSOLETE_PAUSE_ACTIONS:
        observations = [normalized_actions[index][kind] for index in eligible]
        if max(observations.count(item) for item in set(observations)) < 2:
            fail(f"{context} {kind} has no 2-of-3 canonical agreement")

    ic_pause = value["ic_pause"]
    if not isinstance(ic_pause, dict):
        fail(f"{context}.ic_pause must be an object")
    exact_keys(
        ic_pause,
        {
            "method",
            "response",
            "audit_sequence",
            "audit_kind",
            "audit_caller",
            "status_deposits_paused",
            "status_last_audit_sequence",
            "observed_at",
        },
        f"{context}.ic_pause",
    )
    audit_sequence = require_nat(ic_pause, "audit_sequence", f"{context}.ic_pause")
    ic_pause_at = validate_timestamp(require_string(ic_pause, "observed_at", f"{context}.ic_pause"), f"{context}.ic_pause.observed_at")
    if ic_pause_at < started_at or ic_pause_at > ic_live_at or (ic_live_at - ic_pause_at).total_seconds() > 60:
        fail(f"{context} IC pause and live observations must be within one minute")
    observation_times = [*provider_observed_at, ic_pause_at, ic_live_at]
    if (max(observation_times) - min(observation_times)).total_seconds() > 60:
        fail(f"{context} observations must be captured within one minute")
    if (
        ic_pause["method"] != "pause_new_deposits"
        or ic_pause["response"] != "Ok"
        or ic_pause["audit_kind"] not in {"DepositsPaused", "DepositsPauseRepeated"}
        or not PRINCIPAL.fullmatch(str(ic_pause["audit_caller"]))
        or ic_pause["status_deposits_paused"] is not True
        or require_nat(
            ic_pause,
            "status_last_audit_sequence",
            f"{context}.ic_pause",
        )
        != audit_sequence
    ):
        fail(f"{context} IC pause response, audit, and status do not agree")


def validate_obsolete_upgrade_snapshots(
    artifacts: Any,
    manifest_path: Path,
) -> None:
    bridge_status = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_BRIDGE_STATUS_ARTIFACT_KIND,
        "v30 upgrade preflight",
    )
    exact_keys(bridge_status, UPGRADE_STATE_COUNT_FIELDS, "live bridge status")
    for field in UPGRADE_STATE_COUNT_FIELDS:
        require_nat(bridge_status, field, "live bridge status")

    activation = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_ACTIVATION_STATUS_ARTIFACT_KIND,
        "v30 upgrade preflight",
    )
    exact_keys(activation, {"pending_timelock_operations"}, "live activation status")
    if require_nat(
        activation,
        "pending_timelock_operations",
        "live activation status",
    ) != 0:
        fail("v30 upgrade requires zero pending Timelock operations")

    integrity = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_STORAGE_INTEGRITY_ARTIFACT_KIND,
        "v30 upgrade preflight",
    )
    exact_keys(integrity, {"result"}, "live storage integrity")
    if integrity["result"] != "ok":
        fail("v30 upgrade requires storage_integrity_check to return ok")

    ledger = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_LEDGER_BALANCE_ARTIFACT_KIND,
        "v30 upgrade preflight",
    )
    exact_keys(ledger, {"balance_raw"}, "live ledger balance")
    require_nat(ledger, "balance_raw", "live ledger balance")


def validate_preflight(
    details: dict[str, Any],
    binding: dict[str, Any],
    artifacts: Any,
    manifest_path: Path,
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
    live_public_config = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_PUBLIC_CONFIG_ARTIFACT_KIND,
        "preflight",
    )
    recorded_check = required_json_artifact(
        artifacts,
        manifest_path,
        REINSTALL_INSTANCE_CHECK_ARTIFACT_KIND,
        "preflight",
    )
    live_canister_status = required_json_artifact(
        artifacts,
        manifest_path,
        LIVE_CANISTER_STATUS_ARTIFACT_KIND,
        "preflight",
    )
    expected_check = reinstall_instance_check(
        binding["deployment_instance_id"],
        live_public_config,
        live_canister_status,
        binding["bridge_canister_id"],
    )
    if normalized_reinstall_check(recorded_check) != expected_check:
        fail("reinstall instance check does not match the live PublicConfig and reviewed binding")
    summary_check = normalized_reinstall_check(
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
        fail("preflight summary does not match the verified reinstall instance check")
    obsolete_artifact_matches = [
        artifact
        for artifact in artifacts
        if isinstance(artifact, dict)
        and artifact.get("kind")
        in {
            OBSOLETE_STATE_DISPOSITION_ARTIFACT_KIND,
            OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND,
        }
    ]
    if expected_check["replacement_mode"] == OBSOLETE_SCHEMA_UPGRADE:
        validate_obsolete_upgrade_snapshots(artifacts, manifest_path)
        pause_evidence = required_json_artifact(
            artifacts,
            manifest_path,
            OBSOLETE_PAUSE_EVIDENCE_ARTIFACT_KIND,
            "preflight",
        )
        validate_obsolete_pause_evidence(
            pause_evidence,
            binding,
            expected_check,
        )
        if (
            live_canister_status["controller_principals"]
            != details["controller_principals"]
            or require_nat(
                live_canister_status,
                "cycles_balance",
                "live canister status",
            )
            != require_nat(details, "cycles_balance", context)
        ):
            fail("preflight summary differs from the live canister status snapshot")
        if any(
            artifact.get("kind") == OBSOLETE_STATE_DISPOSITION_ARTIFACT_KIND
            for artifact in obsolete_artifact_matches
        ):
            fail("schema upgrade must not include discarded-state evidence")
    elif obsolete_artifact_matches:
        fail("current-schema reinstall must not include obsolete replacement artifacts")


def validate_install(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "install.details"
    base_fields = {"install_mode", "module_sha256", "cycles_balance", "controller_principals"}
    upgrade_fields = {
        "state_counts_before",
        "state_counts_after",
        "schema_version_after",
        "deployment_instance_id_after",
        "storage_integrity_after",
    }
    install_mode = require_string(details, "install_mode", context)
    exact_keys(
        details,
        base_fields | upgrade_fields if install_mode == "upgrade" else base_fields,
        context,
    )
    if details["install_mode"] not in {"install", "upgrade", "reinstall"}:
        fail("staging install_mode must be install, upgrade, or reinstall")
    if details["module_sha256"] != binding["bridge_wasm_sha256"]:
        fail("installed Bridge module does not match local promotion evidence")
    if require_nat(details, "cycles_balance", context) <= 0:
        fail("installed Bridge must retain a positive cycles balance")
    controllers = details["controller_principals"]
    if not isinstance(controllers, list) or not controllers:
        fail("install must retain an explicit controller set")
    if install_mode == "upgrade":
        before = details["state_counts_before"]
        after = details["state_counts_after"]
        if not isinstance(before, dict) or not isinstance(after, dict):
            fail("upgrade state counts must be objects")
        exact_keys(before, UPGRADE_STATE_COUNT_FIELDS, "install state_counts_before")
        exact_keys(after, UPGRADE_STATE_COUNT_FIELDS, "install state_counts_after")
        for field in UPGRADE_STATE_COUNT_FIELDS:
            require_nat(before, field, "install state_counts_before")
            require_nat(after, field, "install state_counts_after")
        if before != after:
            fail("upgrade changed persisted Bridge state counts")
        if require_nat(details, "schema_version_after", context) != CURRENT_STABLE_SCHEMA:
            fail("upgrade did not reach the current stable schema")
        if (
            deployment_instance_hex(
                details["deployment_instance_id_after"],
                "install deployment_instance_id_after",
            )
            != binding["deployment_instance_id"]
        ):
            fail("upgrade changed the deployment instance ID")
        if details["storage_integrity_after"] != "ok":
            fail("upgrade storage integrity check did not return ok")


def validate_initialize(details: dict[str, Any], binding: dict[str, Any]) -> None:
    context = "initialize.details"
    exact_keys(
        details,
        {
            "schema_version",
            "deployment_instance_id",
            "chain_id",
            "ledger_canister_id",
            "index_canister_id",
            "evm_rpc_canister_id",
            "expected_bridge_signer",
            "bridge_address",
            "timelock_address",
            "expected_bridge_runtime_sha256",
            "governance_operator",
            "canister_deposits_paused",
            "storage_integrity",
        },
        context,
    )
    if require_nat(details, "schema_version", context) != CURRENT_STABLE_SCHEMA:
        fail(f"staging must initialize current stable schema v{CURRENT_STABLE_SCHEMA}")
    if require_deployment_instance_id(details, "deployment_instance_id", context) != binding["deployment_instance_id"]:
        fail("initialized deployment instance ID differs from the reviewed binding")
    if details["chain_id"] != binding["chain_id"]:
        fail("initialized Bridge has the wrong chain ID")
    for field in ("bridge_address", "timelock_address", "expected_bridge_signer"):
        require_pattern(details, field, EVM_ADDRESS, context)
        if details[field] != binding[field]:
            fail(f"initialized {field} differs from the reviewed binding")
    require_pattern(details, "expected_bridge_runtime_sha256", EVM_HASH, context)
    if details["expected_bridge_runtime_sha256"] != binding["bridge_runtime_sha256"]:
        fail("initialized Bridge runtime differs from the reviewed binding")
    for field in ("ledger_canister_id", "index_canister_id"):
        if details[field] != binding[field]:
            fail(f"initialized {field} differs from the reviewed binding")
    if details["evm_rpc_canister_id"] != EVM_RPC_CANISTER_ID:
        fail("initialized Bridge does not use the official EVM RPC Canister")
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


def validate_rpc(details: dict[str, Any]) -> None:
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
    "smoke_e2e": validate_smoke_e2e,
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
        "bridge_runtime_template_sha256",
        "bsns_runtime_template_sha256",
        "frontend_profile_sha256",
        "bridge_canister_id",
        "ledger_canister_id",
        "index_canister_id",
        "environment_mode",
        "activation_timelock_delay_seconds",
        "deployment_instance_id",
        "chain_id",
        "bridge_address",
        "bsns_address",
        "timelock_address",
        "bridge_runtime_sha256",
        "bsns_runtime_sha256",
        "expected_bridge_signer",
    }
    exact_keys(binding, expected, "binding")
    require_pattern(binding, "source_commit", GIT_COMMIT, "binding")
    for field in ("local_e2e_sha256", "bridge_wasm_sha256", "frontend_profile_sha256"):
        require_pattern(binding, field, SHA256, "binding")
    for field in ("bridge_runtime_template_sha256", "bsns_runtime_template_sha256"):
        require_pattern(binding, field, EVM_HASH, "binding")
    require_deployment_instance_id(binding, "deployment_instance_id", "binding")
    if require_nat(binding, "chain_id", "binding") != CHAIN_ID:
        fail("binding has the wrong chain ID")
    for field in ("bridge_address", "bsns_address", "timelock_address", "expected_bridge_signer"):
        require_pattern(binding, field, EVM_ADDRESS, "binding")
    for field in ("bridge_runtime_sha256", "bsns_runtime_sha256"):
        require_pattern(binding, field, EVM_HASH, "binding")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        require_pattern(binding, field, PRINCIPAL, "binding")
    if binding["environment_mode"] != ENVIRONMENT_MODE:
        fail("binding is not the short-delay test-only environment")
    if require_nat(binding, "activation_timelock_delay_seconds", "binding") != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("binding has the wrong staging activation delay")
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
    if stage == "preflight":
        validator(details, binding, evidence["artifacts"], manifest_path)
    elif stage in {"install", "initialize", "contracts", "frontend_publish"}:
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
    expected_state = manifest_state(completed)
    expected_complete = len(completed) == len(STAGES)
    if manifest["state"] != expected_state or manifest["complete"] is not expected_complete:
        fail("manifest state does not match its recorded stages")
    if require_complete and not expected_complete:
        fail(f"staging E2E is incomplete: {manifest['state']}")


def manifest_state(completed: list[str]) -> str:
    if len(completed) == len(STAGES):
        return "SHORT_DELAY_COMPLETE"
    if completed and completed[-1] == "smoke_e2e":
        return "SHORT_DELAY_ACTIVE_SMOKE_PASSED"
    return f"AWAITING_{STAGES[len(completed)].upper()}"


def initialize(output: Path, local_evidence_path: Path, profile_path: Path, repo_root: Path | None = None) -> None:
    if output.exists():
        fail(f"refusing to overwrite existing manifest: {output}")
    local = load_object(local_evidence_path)
    profile = load_object(profile_path)
    required_tests = {"full_local_ci", "real_frontend_e2e", "canister_activation", "timelock_delay_enforced", "state_upgrade"}
    if local.get("schema_version") != 7 or set(local.get("tests", {})) != required_tests or any(local["tests"][name] != "passed" for name in required_tests):
        fail("local promotion evidence is not a complete schema v7 pass")
    upgrade = local.get("state_upgrade")
    if not isinstance(upgrade, dict) or upgrade.get("verified") is not True:
        fail("local promotion evidence has no verified same-Wasm upgrade")
    before = upgrade.get("before")
    after = upgrade.get("after")
    for label, state in (("before", before), ("after", after)):
        if not isinstance(state, dict) or not isinstance(state.get("status"), dict):
            fail(f"local promotion evidence state_upgrade.{label} has no status")
        try:
            schema_version = int(state["status"].get("schema_version"))
        except (TypeError, ValueError):
            fail(f"local promotion evidence state_upgrade.{label} has no stable schema")
        if schema_version != CURRENT_STABLE_SCHEMA:
            fail(
                "local promotion evidence "
                f"state_upgrade.{label} must use stable schema v{CURRENT_STABLE_SCHEMA}"
            )
    if local.get("environment_mode") != ENVIRONMENT_MODE or local.get("activation_timelock_delay_seconds") != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("local promotion evidence is not bound to the five-minute staging policy")
    if profile.get("environment") != "sepolia-staging" or profile.get("testOnly") is not True or profile.get("chainId") != CHAIN_ID:
        fail("frontend profile is not the Base Sepolia test-only profile")
    if profile.get("evmRpcCanisterId") != EVM_RPC_CANISTER_ID:
        fail("frontend profile does not use the official EVM RPC Canister")
    if profile.get("environmentMode") != ENVIRONMENT_MODE or profile.get("activationTimelockDelaySeconds") != ACTIVATION_TIMELOCK_DELAY_SECONDS:
        fail("frontend profile is not bound to the five-minute staging policy")
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
        "bridge_runtime_template_sha256": local.get("bridge_runtime_template_sha256"),
        "bsns_runtime_template_sha256": local.get("bsns_runtime_template_sha256"),
        "frontend_profile_sha256": digest(profile_path),
        "bridge_canister_id": profile.get("bridgeCanisterId"),
        "ledger_canister_id": profile.get("ledgerCanisterId"),
        "index_canister_id": profile.get("indexCanisterId"),
        "environment_mode": local.get("environment_mode"),
        "activation_timelock_delay_seconds": local.get("activation_timelock_delay_seconds"),
        "deployment_instance_id": profile.get("deploymentInstanceId"),
        "chain_id": profile.get("chainId"),
        "bridge_address": profile.get("bridgeAddress"),
        "bsns_address": profile.get("bsnsAddress"),
        "timelock_address": profile.get("timelockAddress"),
        "bridge_runtime_sha256": profile.get("bridgeRuntimeHash"),
        "bsns_runtime_sha256": profile.get("bsnsRuntimeHash"),
        "expected_bridge_signer": profile.get("expected_bridge_signer"),
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
    completed = [stage for stage in STAGES if manifest["stages"][stage] is not None]
    manifest["state"] = manifest_state(completed)
    write_object(manifest_path, manifest)


def capture_obsolete_pause_evidence(
    capture_config_path: Path,
    output_path: Path,
    profile_path: Path,
    live_public_config_path: Path,
    live_canister_status_path: Path,
    run_collector: Any = subprocess.run,
) -> None:
    if output_path.exists():
        fail(f"refusing to overwrite existing pause evidence: {output_path}")
    profile = load_object(profile_path)
    live_public_config = load_object(live_public_config_path)
    live_canister_status = load_object(live_canister_status_path)
    command = [
        "node",
        str(OBSOLETE_PAUSE_COLLECTOR_PATH),
        str(profile_path),
        str(capture_config_path),
    ]
    try:
        completed = run_collector(
            command,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError) as error:
        status = getattr(error, "returncode", "unavailable")
        fail(f"live pause collector failed with exit status {status}")
    try:
        value = json.loads(completed.stdout)
    except (AttributeError, json.JSONDecodeError) as error:
        fail(f"live pause collector returned invalid JSON: {error}")
    if not isinstance(value, dict):
        fail("live pause collector must return a JSON object")
    binding = {
        "bridge_canister_id": profile.get("bridgeCanisterId"),
        "bridge_address": profile.get("bridgeAddress"),
    }
    ic_live = value.get("ic_live")
    if not isinstance(ic_live, dict):
        fail("live pause collector did not return post-pause IC observations")
    expected_check = reinstall_instance_check(
        profile.get("deploymentInstanceId"),
        {
            "schema_version": ic_live.get("live_schema_version"),
            "deployment_instance_id": ic_live.get("previous_deployment_instance_id"),
        },
        {"module_hash": ic_live.get("live_module_hash")},
        str(profile.get("bridgeCanisterId")),
    )
    if expected_check["replacement_mode"] != OBSOLETE_SCHEMA_UPGRADE:
        fail("obsolete pause evidence is only valid for the reviewed v30 upgrade")
    validate_obsolete_pause_evidence(value, binding, expected_check)
    preflight_check = reinstall_instance_check(
        profile.get("deploymentInstanceId"),
        live_public_config,
        live_canister_status,
        str(profile.get("bridgeCanisterId")),
    )
    if preflight_check != expected_check:
        fail("post-pause IC observations differ from the reviewed preflight snapshots")
    write_new_object_atomically(output_path, value)


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
    pause_parser = subparsers.add_parser("capture-obsolete-pause")
    pause_parser.add_argument("capture_config", type=Path)
    pause_parser.add_argument("output", type=Path)
    pause_parser.add_argument("frontend_profile", type=Path)
    pause_parser.add_argument("live_public_config", type=Path)
    pause_parser.add_argument("live_canister_status", type=Path)
    args = parser.parse_args()
    try:
        if args.command == "init":
            initialize(args.manifest, args.local_e2e, args.frontend_profile, args.repo_root)
            print(f"initialized {args.manifest}")
        elif args.command == "record":
            record(args.manifest, args.evidence)
            manifest = load_object(args.manifest)
            print(f"recorded {args.evidence}; state={manifest['state']}")
        elif args.command == "capture-obsolete-pause":
            capture_obsolete_pause_evidence(
                args.capture_config,
                args.output,
                args.frontend_profile,
                args.live_public_config,
                args.live_canister_status,
            )
            print(f"captured obsolete pause evidence: {args.output}")
        else:
            manifest = load_object(args.manifest)
            validate_manifest(manifest, args.manifest, require_complete=not args.allow_incomplete, verify_files=True)
            print(f"verified {args.manifest}; state={manifest['state']}")
    except EvidenceError as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
