#!/usr/bin/env python3
"""Fail-closed evidence recorder for the live Base Sepolia EVM RPC rehearsal.

This program never submits an IC or EVM transaction. Operators run the documented
manual steps, export one JSON observation per scenario, and use this recorder to
validate and bind those observations into a resumable manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit


OFFICIAL_EVM_RPC_CANISTER_ID = "7hfb6-caaaa-aaaar-qadga-cai"
BASE_SEPOLIA_CHAIN_ID = 84532
SCENARIOS = (
    "preflight",
    "deposit_mint",
    "withdrawal_release",
    "bad_fee_refund",
    "canonical_receipt",
    "single_provider_failure",
    "quorum_loss",
    "nonce_known",
    "nonce_conflict",
    "final_pause",
)
REQUIRED_ARTIFACTS = {
    "preflight": {"bridge", "base", "audit", "module"},
    "deposit_mint": {"bridge", "base", "ledger", "audit"},
    "withdrawal_release": {"bridge", "base", "ledger", "audit"},
    "bad_fee_refund": {"bridge", "base", "ledger", "audit"},
    "canonical_receipt": {"bridge", "base", "audit"},
    "single_provider_failure": {"bridge", "audit", "fault"},
    "quorum_loss": {"bridge", "audit", "fault"},
    "nonce_known": {"bridge", "audit"},
    "nonce_conflict": {"bridge", "audit"},
    "final_pause": {"bridge", "base", "audit"},
}
CROSS_ARTIFACT_BINDINGS = {
    "preflight": {"observed_bridge_contract": {"bridge", "base"}, "observed_chain_id": {"bridge", "base"}, "base_bridge_signer": {"base"}, "canister_chain_key_signer": {"bridge"}, "bridge_canister_module_sha256": {"module"}},
    "deposit_mint": {"deposit_id": {"bridge", "base", "audit"}, "ledger_block_index": {"bridge", "ledger"}, "mint_transaction_hash": {"bridge", "base"}, "safe_block_hash": {"bridge", "base"}},
    "withdrawal_release": {"withdrawal_id": {"bridge", "base", "audit"}, "ledger_block_index": {"bridge", "ledger"}, "request_transaction_hash": {"bridge", "base"}, "acknowledge_transaction_hash": {"bridge", "base"}, "request_safe_block_hash": {"bridge", "base"}, "acknowledgement_safe_block_hash": {"bridge", "base"}},
    "bad_fee_refund": {"withdrawal_id": {"bridge", "base", "audit"}, "new_ledger_fee": {"bridge", "ledger"}, "cancel_release_transaction_hash": {"bridge", "base"}, "refund_transaction_hash": {"bridge", "base"}, "safe_block_hash": {"bridge", "base"}},
    "canonical_receipt": {"transaction_hash": {"bridge", "base"}, "receipt_block_hash": {"bridge", "base"}, "canonical_block_hash": {"bridge", "base"}, "confirmed_head_block_number": {"bridge", "base"}},
    "single_provider_failure": {"configured_provider_count": {"fault"}, "required_provider_threshold": {"fault"}, "injected_provider_failures": {"fault"}, "fault_injection_reference": {"fault"}, "bridge_operation_continued": {"bridge", "audit"}},
    "quorum_loss": {"configured_provider_count": {"fault"}, "required_provider_threshold": {"fault"}, "injected_provider_failures": {"fault"}, "fault_injection_reference": {"fault"}, "stop_reason": {"bridge", "audit"}, "ledger_call_performed": {"bridge", "audit"}},
    "nonce_known": {"local_transaction_hash": {"bridge", "audit"}, "provider_agreement": {"bridge", "audit"}, "resulting_state": {"bridge", "audit"}},
    "nonce_conflict": {"resulting_stop_reason": {"bridge", "audit"}, "deposits_paused": {"bridge", "audit"}, "automatically_resigned": {"bridge", "audit"}},
    "final_pause": {"base_deposits_paused": {"base", "audit"}, "base_withdrawals_paused": {"base", "audit"}, "canister_deposits_paused": {"bridge", "audit"}, "safe_block_hash": {"base", "audit"}},
}
HEX_32 = re.compile(r"^0x[0-9a-fA-F]{64}$")
HEX_20 = re.compile(r"^0x[0-9a-fA-F]{40}$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
PRINCIPAL = re.compile(r"^[a-z0-9-]{5,63}$")
REHEARSAL_ID = re.compile(r"^[a-z0-9][a-z0-9-]{7,63}$")
RPC_AUDIT_SCENARIOS = frozenset(SCENARIOS) - {"quorum_loss"}
RPC_DECISION_SCENARIOS = frozenset({"single_provider_failure", "quorum_loss", "nonce_conflict"})
RPC_AUDIT_METHODS = {
    "preflight": {"multi_request"},
    "deposit_mint": {"eth_sendRawTransaction", "eth_sendRawTransaction+multi_request", "eth_getTransactionReceipt+eth_getBlockByNumber"},
    "withdrawal_release": {"multi_request", "eth_sendRawTransaction", "eth_sendRawTransaction+multi_request", "eth_getTransactionReceipt+eth_getBlockByNumber"},
    "bad_fee_refund": {"eth_sendRawTransaction", "eth_sendRawTransaction+multi_request", "eth_getTransactionReceipt+eth_getBlockByNumber"},
    "canonical_receipt": {"multi_request", "eth_getTransactionReceipt+eth_getBlockByNumber"},
    "single_provider_failure": {"multi_request"},
    "nonce_known": {"eth_sendRawTransaction+multi_request"},
    "nonce_conflict": {"eth_sendRawTransaction", "eth_sendRawTransaction+multi_request"},
    "final_pause": {"multi_request"},
}


class InvalidEvidence(ValueError):
    pass


def fail(message: str) -> None:
    raise InvalidEvidence(message)


def load_object(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read JSON object {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{path} must contain one JSON object")
    return value


def exact_keys(value: dict[str, Any], expected: set[str], context: str) -> None:
    actual = set(value)
    if actual != expected:
        fail(
            f"{context} fields differ: missing={sorted(expected - actual)}, "
            f"unknown={sorted(actual - expected)}"
        )


def valid_timestamp(value: Any, context: str) -> None:
    if not isinstance(value, str) or not value.endswith("Z"):
        fail(f"{context} must be an RFC 3339 UTC timestamp")
    try:
        datetime.fromisoformat(value[:-1] + "+00:00")
    except ValueError:
        fail(f"{context} must be an RFC 3339 UTC timestamp")


def credential_free_https_url(value: Any) -> tuple[str, str]:
    if not isinstance(value, str):
        fail("RPC URL must be a string")
    parsed = urlsplit(value)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or "." not in parsed.hostname
        or not any(character.isalpha() for character in parsed.hostname)
        or any(not label or label.startswith("-") or label.endswith("-") for label in parsed.hostname.split("."))
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or parsed.netloc.endswith(":")
    ):
        fail("RPC URLs must be credential-free HTTPS URLs without query or fragment")
    try:
        port = parsed.port
    except ValueError:
        fail("RPC URL port is invalid")
    if port == 0:
        fail("RPC URL port is invalid")
    path_segments = [segment for segment in parsed.path.split("/") if segment]
    public_path_segments = {"rpc", "v1", "v2", "ethereum", "base", "base-mainnet", "base-sepolia"}
    for segment in path_segments:
        lowered = segment.lower()
        if lowered not in public_path_segments:
            fail("RPC URL path is not in the public credential-free allowlist")
    normalized = f"https://{parsed.hostname.lower()}"
    if port is not None:
        normalized += f":{port}"
    normalized += parsed.path.rstrip("/") or "/"
    return normalized, parsed.hostname.lower()


def validate_config(config: dict[str, Any]) -> dict[str, Any]:
    exact_keys(
        config,
        {
            "schema_version",
            "rehearsal_id",
            "test_only",
            "ic_network",
            "base_chain_id",
            "evm_rpc_canister_id",
            "bridge_canister_id",
            "ledger_canister_id",
            "index_canister_id",
            "bridge_contract",
            "expected_bridge_signer",
            "bridge_canister_wasm_sha256",
            "bridge_runtime_bytecode_sha256",
            "rpc_urls",
        },
        "config",
    )
    if config["schema_version"] != 1 or config["test_only"] is not True:
        fail("only schema v1 test-only rehearsal configs are accepted")
    if not isinstance(config["rehearsal_id"], str) or not REHEARSAL_ID.fullmatch(
        config["rehearsal_id"]
    ):
        fail("rehearsal_id must be a lowercase, hyphenated identifier")
    if config["ic_network"] != "ic":
        fail("the live rehearsal must target the IC network")
    if config["base_chain_id"] != BASE_SEPOLIA_CHAIN_ID:
        fail(f"the live rehearsal must target Base Sepolia ({BASE_SEPOLIA_CHAIN_ID})")
    if config["evm_rpc_canister_id"] != OFFICIAL_EVM_RPC_CANISTER_ID:
        fail("the live rehearsal must use the official EVM RPC Canister")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        if not isinstance(config[field], str) or not PRINCIPAL.fullmatch(config[field]):
            fail(f"{field} is not a valid textual principal")
    if not isinstance(config["bridge_contract"], str) or not HEX_20.fullmatch(
        config["bridge_contract"]
    ):
        fail("bridge_contract must be a 20-byte hex address")
    if not isinstance(config["expected_bridge_signer"], str) or not HEX_20.fullmatch(
        config["expected_bridge_signer"]
    ):
        fail("expected_bridge_signer must be a 20-byte hex address")
    for field in ("bridge_canister_wasm_sha256", "bridge_runtime_bytecode_sha256"):
        if not isinstance(config[field], str) or not SHA256.fullmatch(config[field]):
            fail(f"{field} must be a lowercase SHA-256 digest")
    urls = config["rpc_urls"]
    if not isinstance(urls, list) or len(urls) != 3:
        fail("exactly three custom RPC URLs are required")
    normalized_and_hosts = [credential_free_https_url(url) for url in urls]
    normalized = [item[0] for item in normalized_and_hosts]
    if len(set(normalized)) != 3:
        fail("the three custom RPC URL strings must be distinct")
    return {
        "schema_version": 1,
        "rehearsal_id": config["rehearsal_id"],
        "test_only": True,
        "ic_network": "ic",
        "base_chain_id": BASE_SEPOLIA_CHAIN_ID,
        "evm_rpc_canister_id": OFFICIAL_EVM_RPC_CANISTER_ID,
        "bridge_canister_id": config["bridge_canister_id"],
        "ledger_canister_id": config["ledger_canister_id"],
        "index_canister_id": config["index_canister_id"],
        "bridge_contract": config["bridge_contract"].lower(),
        "expected_bridge_signer": config["expected_bridge_signer"].lower(),
        "bridge_canister_wasm_sha256": config["bridge_canister_wasm_sha256"],
        "bridge_runtime_bytecode_sha256": config["bridge_runtime_bytecode_sha256"],
        # URLs are deliberately reduced to host and digest in the evidence manifest.
        "rpc_endpoints": [
            {
                "host": host,
                "url_sha256": hashlib.sha256(url.encode()).hexdigest(),
            }
            for url, (_, host) in zip(urls, normalized_and_hosts, strict=True)
        ],
    }


def validate_binding(binding: dict[str, Any]) -> None:
    exact_keys(
        binding,
        {
            "schema_version",
            "rehearsal_id",
            "test_only",
            "ic_network",
            "base_chain_id",
            "evm_rpc_canister_id",
            "bridge_canister_id",
            "ledger_canister_id",
            "index_canister_id",
            "bridge_contract",
            "expected_bridge_signer",
            "bridge_canister_wasm_sha256",
            "bridge_runtime_bytecode_sha256",
            "rpc_endpoints",
        },
        "manifest binding",
    )
    if (
        binding["schema_version"] != 1
        or binding["test_only"] is not True
        or binding["ic_network"] != "ic"
        or binding["base_chain_id"] != BASE_SEPOLIA_CHAIN_ID
        or binding["evm_rpc_canister_id"] != OFFICIAL_EVM_RPC_CANISTER_ID
    ):
        fail("manifest has an invalid schema, network, chain, or canister binding")
    if not isinstance(binding["rehearsal_id"], str) or not REHEARSAL_ID.fullmatch(binding["rehearsal_id"]):
        fail("manifest rehearsal_id is invalid")
    for field in ("bridge_canister_id", "ledger_canister_id", "index_canister_id"):
        if not isinstance(binding[field], str) or not PRINCIPAL.fullmatch(binding[field]):
            fail(f"manifest {field} is invalid")
    for field in ("bridge_contract", "expected_bridge_signer"):
        if not isinstance(binding[field], str) or not HEX_20.fullmatch(binding[field]):
            fail(f"manifest {field} is invalid")
    for field in ("bridge_canister_wasm_sha256", "bridge_runtime_bytecode_sha256"):
        if not isinstance(binding[field], str) or not SHA256.fullmatch(binding[field]):
            fail(f"manifest {field} is invalid")
    endpoints = binding["rpc_endpoints"]
    if not isinstance(endpoints, list) or len(endpoints) != 3:
        fail("manifest must bind exactly three RPC endpoint digests")
    hosts: list[str] = []
    digests: list[str] = []
    for endpoint in endpoints:
        if not isinstance(endpoint, dict):
            fail("manifest RPC endpoint binding must be an object")
        exact_keys(endpoint, {"host", "url_sha256"}, "manifest RPC endpoint")
        if not isinstance(endpoint["host"], str) or not endpoint["host"]:
            fail("manifest RPC endpoint host is invalid")
        if not isinstance(endpoint["url_sha256"], str) or not SHA256.fullmatch(endpoint["url_sha256"]):
            fail("manifest RPC endpoint digest is invalid")
        hosts.append(endpoint["host"])
        digests.append(endpoint["url_sha256"])
    if len(set(digests)) != 3:
        fail("manifest RPC endpoint URL digests must be distinct")


def git_value(root: Path, *args: str) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), *args],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return result.stdout.strip()


def source_tree_sha256(root: Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), "archive", "HEAD"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    return hashlib.sha256(result.stdout).hexdigest()


def now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    temporary.replace(path)


def initial_manifest(config: dict[str, Any], root: Path) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "kind": "base-sepolia-evm-rpc-canister-rehearsal",
        "state": "AWAITING_PREFLIGHT",
        "created_at": now(),
        "updated_at": now(),
        "source": {
            "revision": git_value(root, "rev-parse", "HEAD"),
            "source_tree_sha256": source_tree_sha256(root),
            "worktree_status_sha256": hashlib.sha256(
                git_value(root, "status", "--short", "--untracked-files=all").encode()
            ).hexdigest(),
        },
        "binding": config,
        "scenarios": {name: None for name in SCENARIOS},
        "complete": False,
        "guarantee_boundary": {
            "provider_operator_or_infrastructure_audited": False,
            "external_assumption": (
                "The official EVM RPC Canister and configured provider quorum return the "
                "canonical Safe Base Sepolia chain."
            ),
        },
    }


def validate_manifest_envelope(manifest: dict[str, Any]) -> None:
    exact_keys(
        manifest,
        {"schema_version", "kind", "state", "created_at", "updated_at", "source", "binding", "scenarios", "complete", "guarantee_boundary"},
        "rehearsal manifest",
    )
    if manifest["schema_version"] != 1 or manifest["kind"] != "base-sepolia-evm-rpc-canister-rehearsal":
        fail("manifest is not a v1 EVM RPC rehearsal manifest")
    valid_timestamp(manifest["created_at"], "manifest.created_at")
    valid_timestamp(manifest["updated_at"], "manifest.updated_at")
    source = manifest["source"]
    if not isinstance(source, dict):
        fail("manifest source must be an object")
    exact_keys(source, {"revision", "source_tree_sha256", "worktree_status_sha256"}, "manifest source")
    if not isinstance(source["revision"], str) or not re.fullmatch(r"[0-9a-f]{40,64}", source["revision"]):
        fail("manifest source revision is invalid")
    if not isinstance(source["worktree_status_sha256"], str) or not SHA256.fullmatch(source["worktree_status_sha256"]):
        fail("manifest worktree status digest is invalid")
    if not isinstance(source["source_tree_sha256"], str) or not SHA256.fullmatch(source["source_tree_sha256"]):
        fail("manifest source tree digest is invalid")
    boundary = manifest["guarantee_boundary"]
    if not isinstance(boundary, dict):
        fail("manifest guarantee boundary must be an object")
    exact_keys(boundary, {"provider_operator_or_infrastructure_audited", "external_assumption"}, "manifest guarantee boundary")
    if boundary["provider_operator_or_infrastructure_audited"] is not False or not isinstance(boundary["external_assumption"], str) or not boundary["external_assumption"]:
        fail("manifest guarantee boundary is invalid")


def validate_common(
    evidence: dict[str, Any], binding: dict[str, Any], expected_scenario: str
) -> None:
    common = {
        "schema_version",
        "rehearsal_id",
        "scenario",
        "observed_at",
        "test_assets_only",
        "external_calls_performed",
        "through_evm_rpc_canister",
        "used_test_double",
        "ic_network",
        "base_chain_id",
        "evm_rpc_canister_id",
        "bridge_canister_id",
        "request_sha256",
        "response_sha256",
        "result",
        "details",
        "canister_audit",
        "canister_decision",
        "artifacts",
    }
    exact_keys(evidence, common, f"{expected_scenario} evidence")
    if evidence["schema_version"] != 1:
        fail("scenario evidence schema_version must be 1")
    expected = {
        "rehearsal_id": binding["rehearsal_id"],
        "scenario": expected_scenario,
        "test_assets_only": True,
        "external_calls_performed": True,
        "through_evm_rpc_canister": True,
        "used_test_double": False,
        "ic_network": "ic",
        "base_chain_id": BASE_SEPOLIA_CHAIN_ID,
        "evm_rpc_canister_id": OFFICIAL_EVM_RPC_CANISTER_ID,
        "bridge_canister_id": binding["bridge_canister_id"],
        "result": "passed",
    }
    for field, value in expected.items():
        if evidence[field] != value:
            fail(f"{expected_scenario}.{field} must equal {value!r}")
    valid_timestamp(evidence["observed_at"], f"{expected_scenario}.observed_at")
    for field in ("request_sha256", "response_sha256"):
        if not isinstance(evidence[field], str) or not SHA256.fullmatch(evidence[field]):
            fail(f"{expected_scenario}.{field} must be a lowercase SHA-256 digest")
    if not isinstance(evidence["details"], dict):
        fail(f"{expected_scenario}.details must be an object")
    validate_canister_audit(evidence["canister_audit"], binding, expected_scenario, evidence["details"])
    validate_canister_decision(evidence["canister_decision"], expected_scenario, evidence["details"])
    if expected_scenario == "nonce_conflict" and (
        evidence["canister_audit"] is None
        or evidence["canister_decision"] is None
        or evidence["canister_decision"]["transaction_hash"]
        != evidence["canister_audit"]["transaction_hash"]
    ):
        fail("nonce conflict audit and decision must bind the same local transaction hash")
    artifacts = evidence["artifacts"]
    if not isinstance(artifacts, list) or not artifacts:
        fail(f"{expected_scenario}.artifacts must be a non-empty array")
    kinds = set()
    bound_by_kind: dict[str, set[str]] = {}
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            fail(f"{expected_scenario} artifact reference must be an object")
        exact_keys(artifact, {"kind", "path", "sha256", "bindings"}, "artifact reference")
        if artifact["kind"] not in {"bridge", "base", "ledger", "audit", "module", "fault"}:
            fail("artifact reference kind is invalid")
        path = artifact["path"]
        if (
            not isinstance(path, str)
            or Path(path).is_absolute()
            or ".." in Path(path).parts
            or not path.startswith("artifacts/")
        ):
            fail("artifact reference path must remain under artifacts/")
        if not isinstance(artifact["sha256"], str) or not SHA256.fullmatch(artifact["sha256"]):
            fail("artifact reference digest is invalid")
        if not isinstance(artifact["bindings"], dict) or any(
            not isinstance(field, str) or not isinstance(pointer, str) or not pointer.startswith("/")
            for field, pointer in artifact["bindings"].items()
        ):
            fail("artifact bindings must map detail fields to JSON pointers")
        if (
            expected_scenario == "preflight"
            and artifact["kind"] == "base"
            and artifact["bindings"].get("observed_chain_id")
            != "/@transport/observed_chain_id"
        ):
            fail("Base observed_chain_id must bind to the fixed helper chain-id response")
        kinds.add(artifact["kind"])
        bound_by_kind.setdefault(artifact["kind"], set()).update(artifact["bindings"])
    if not REQUIRED_ARTIFACTS[expected_scenario].issubset(kinds):
        fail(f"{expected_scenario} is missing required raw artifact kinds")
    for field, required_kinds in CROSS_ARTIFACT_BINDINGS[expected_scenario].items():
        actual_kinds = {kind for kind, fields in bound_by_kind.items() if field in fields}
        if not required_kinds.issubset(actual_kinds):
            fail(f"{expected_scenario}.{field} is not cross-bound by {sorted(required_kinds)}")


def require_hex(details: dict[str, Any], field: str, pattern: re.Pattern[str]) -> None:
    if not isinstance(details.get(field), str) or not pattern.fullmatch(details[field]):
        fail(f"details.{field} has an invalid hex value")


def require_nat(details: dict[str, Any], field: str, positive: bool = False) -> int:
    value = details.get(field)
    if not isinstance(value, int) or isinstance(value, bool) or value < (1 if positive else 0):
        fail(f"details.{field} must be a {'positive' if positive else 'non-negative'} integer")
    return value


def validate_canister_audit(value: Any, binding: dict[str, Any], scenario: str, details: dict[str, Any]) -> None:
    if scenario not in RPC_AUDIT_SCENARIOS:
        if value is not None:
            fail(f"{scenario}.canister_audit must be null when no quorum observation completed")
        return
    if not isinstance(value, dict):
        fail(f"{scenario}.canister_audit must be derived from a Canister audit event")
    exact_keys(value, {"evm_rpc_canister_id", "call_method", "request_digest", "quorum_response_digest", "safe_block_number", "safe_block_hash", "transaction_hash"}, f"{scenario}.canister_audit")
    if (
        value["evm_rpc_canister_id"] != binding["evm_rpc_canister_id"]
        or value["call_method"] not in RPC_AUDIT_METHODS[scenario]
    ):
        fail("Canister audit event is bound to the wrong EVM RPC Canister or Candid method")
    for field in ("request_digest", "quorum_response_digest"):
        if not isinstance(value[field], str) or not SHA256.fullmatch(value[field]):
            fail(f"canister_audit.{field} must be a lowercase SHA-256 digest")
    require_nat(value, "safe_block_number", positive=True)
    require_hex(value, "safe_block_hash", HEX_32)
    if value["transaction_hash"] is not None:
        require_hex(value, "transaction_hash", HEX_32)
    if value["call_method"] != "multi_request" and value["transaction_hash"] is None:
        fail("transaction RPC audit must bind its local transaction hash")
    safe_hashes = {str(v).lower() for k, v in details.items() if "safe_block_hash" in k and isinstance(v, str)}
    if safe_hashes and value["safe_block_hash"].lower() not in safe_hashes:
        fail("Canister audit Safe hash is not bound to the scenario")
    tx_hashes = {str(v).lower() for k, v in details.items() if "transaction_hash" in k and isinstance(v, str)}
    if tx_hashes and (value["transaction_hash"] is None or value["transaction_hash"].lower() not in tx_hashes):
        fail("Canister audit transaction hash is not bound to the scenario")


def normalized_rpc_audits(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, list):
        return [found for item in value for found in normalized_rpc_audits(item)]
    if not isinstance(value, dict):
        return []
    candidate = value.get("EvmRpcObservation", value)
    required = {"evm_rpc_canister_id", "call_method", "request_digest", "quorum_response_digest", "safe_block_number", "safe_block_hash", "transaction_hash"}
    if isinstance(candidate, dict) and required.issubset(candidate):
        normalized = dict(candidate)
        for field in ("request_digest", "quorum_response_digest"):
            raw = normalized[field]
            if isinstance(raw, list) and all(isinstance(byte, int) and 0 <= byte <= 255 for byte in raw):
                normalized[field] = bytes(raw).hex()
        for field in ("safe_block_hash", "transaction_hash"):
            raw = normalized[field]
            if field == "transaction_hash" and isinstance(raw, list):
                if raw == []:
                    raw = None
                elif len(raw) == 1 and isinstance(raw[0], list):
                    raw = raw[0]
                normalized[field] = raw
            if isinstance(raw, list) and all(isinstance(byte, int) and 0 <= byte <= 255 for byte in raw):
                normalized[field] = "0x" + bytes(raw).hex()
        if isinstance(normalized["safe_block_number"], str) and normalized["safe_block_number"].isdigit():
            normalized["safe_block_number"] = int(normalized["safe_block_number"])
        return [normalized]
    return [found for item in value.values() for found in normalized_rpc_audits(item)]


def normalized_rpc_decisions(value: Any) -> list[dict[str, Any]]:
    if isinstance(value, list):
        return [found for item in value for found in normalized_rpc_decisions(item)]
    if not isinstance(value, dict):
        return []
    event_sequence = value.get("sequence")
    event_timestamp = value.get("timestamp_ns")
    kind = value.get("kind")
    candidate = kind.get("EvmRpcDecision") if isinstance(kind, dict) and "EvmRpcDecision" in kind else value.get("EvmRpcDecision", value)
    required = {
        "kind", "operation", "configured_provider_count", "required_threshold",
        "stop_reason", "ledger_call_performed", "bridge_operation_continued",
        "deposits_paused", "automatically_resigned", "transaction_hash",
    }
    if isinstance(candidate, dict) and required.issubset(candidate):
        normalized = dict(candidate)
        raw = normalized["transaction_hash"]
        if isinstance(raw, list):
            if raw == []:
                raw = None
            elif len(raw) == 1 and isinstance(raw[0], list):
                raw = raw[0]
        if isinstance(raw, list) and all(isinstance(byte, int) and 0 <= byte <= 255 for byte in raw):
            raw = "0x" + bytes(raw).hex()
        normalized["transaction_hash"] = raw
        for raw_value, name in ((event_sequence, "sequence"), (event_timestamp, "timestamp_ns")):
            if isinstance(raw_value, str) and raw_value.isdigit():
                raw_value = int(raw_value)
            if not isinstance(raw_value, int):
                break
            if name == "sequence":
                event_sequence = raw_value
            else:
                event_timestamp = raw_value
        else:
            return [{"sequence": event_sequence, "timestamp_ns": event_timestamp, "decision": normalized}]
        return []
    return [found for item in value.values() for found in normalized_rpc_decisions(item)]


def validate_canister_decision(value: Any, scenario: str, details: dict[str, Any]) -> None:
    if scenario not in RPC_DECISION_SCENARIOS:
        if value is not None:
            fail(f"{scenario}.canister_decision must be null")
        return
    if not isinstance(value, dict):
        fail(f"{scenario}.canister_decision must be derived from EvmRpcDecision")
    exact_keys(value, {
        "kind", "operation", "configured_provider_count", "required_threshold",
        "stop_reason", "ledger_call_performed", "bridge_operation_continued",
        "deposits_paused", "automatically_resigned", "transaction_hash",
    }, f"{scenario}.canister_decision")
    if value["configured_provider_count"] != 3 or value["required_threshold"] != 2:
        fail("Canister decision has the wrong provider threshold")
    if scenario == "single_provider_failure":
        if value["kind"] != "QuorumContinued" or value["operation"] != "refresh_base_observation" or value["stop_reason"] is not None or value["ledger_call_performed"] is not False or value["bridge_operation_continued"] is not True:
            fail("single provider failure lacks a threshold continuation decision")
    elif scenario == "quorum_loss":
        if value["kind"] != "QuorumLoss" or value["operation"] != "notify_withdrawal" or value["stop_reason"] != details["stop_reason"] or value["ledger_call_performed"] is not False or value["bridge_operation_continued"] is not False:
            fail("quorum loss decision is not fail closed before Ledger")
    elif scenario == "nonce_conflict":
        if value["kind"] != "NonceConflict" or value["operation"] != "broadcast_evm_operation" or value["stop_reason"] != "NonceConflict" or value["deposits_paused"] is not True or value["automatically_resigned"] is not False or not isinstance(value["transaction_hash"], str) or not HEX_32.fullmatch(value["transaction_hash"]):
            fail("nonce conflict decision does not pause without re-signing")


def json_pointer(value: Any, pointer: str) -> Any:
    current = value
    for raw in pointer.split("/")[1:]:
        token = raw.replace("~1", "/").replace("~0", "~")
        if isinstance(current, dict) and token in current:
            current = current[token]
        elif isinstance(current, list) and token.isdigit() and int(token) < len(current):
            current = current[int(token)]
        else:
            fail(f"artifact JSON pointer does not exist: {pointer}")
    return current


def validate_capture_command(kind: str, tool: str, argv: list[str], binding: dict[str, Any]) -> None:
    if kind == "fault":
        if tool != "fault-injection-recorder" or len(argv) != 2 or any(not SHA256.fullmatch(value) for value in argv):
            fail("fault artifact must come from the dedicated fault-injection recorder")
        return
    if tool == "dfx":
        expected_prefix = ["canister", "status"] if kind == "module" else ["canister", "call"]
        if kind == "base" or len(argv) < 6 or argv[:2] != expected_prefix:
            fail("dfx artifacts must be fixed canister call/status captures")
        expected_canister = binding["ledger_canister_id"] if kind == "ledger" else binding["bridge_canister_id"]
        if argv[2] != expected_canister or argv.count("--network") != 1:
            fail("dfx artifact targets the wrong canister or network")
        if any(item.startswith("--network=") for item in argv):
            fail("dfx network flag must use the fixed split form exactly once")
        network_index = argv.index("--network")
        if network_index + 1 >= len(argv) or argv[network_index + 1] != "ic":
            fail("dfx artifact must target the IC network")
        if argv.count("--output") != 1 or any(item.startswith("--output=") for item in argv):
            fail("dfx output flag must use the fixed split form exactly once")
        if argv.index("--output") + 1 >= len(argv) or argv[argv.index("--output") + 1] != "json":
            fail("dfx artifact must request raw JSON output")
        forbidden = {"--identity", "--wallet", "--host", "--insecure", "--yes"}
        if any(item in forbidden or any(item.startswith(flag + "=") for flag in forbidden) for item in argv):
            fail("dfx artifact contains an operator-controlled routing or identity flag")
        if kind == "module":
            if len(argv) != 7:
                fail("module artifact must be one fixed dfx canister status capture")
        else:
            method = argv[3]
            allowed_methods = {
                "ledger": {"icrc3_get_blocks", "icrc1_balance_of", "icrc1_fee"},
                "audit": {"get_audit_events"},
                "bridge": {"get_public_config", "get_bridge_status", "get_deposit", "get_withdrawals"},
            }
            if method not in allowed_methods[kind]:
                fail(f"dfx artifact method is not allowed for {kind}: {method}")
    elif tool == "cast":
        if kind != "base" or not argv or argv[0] not in {"receipt", "block", "call"}:
            fail("cast artifacts must be Base receipt, block, or call captures")
        if "--chain" in argv or "--json" in argv or any(
            item == "--rpc-url" or item.startswith("--rpc-url=") for item in argv
        ):
            fail("cast network and output flags are controlled by the fixed capture helper")
        if argv[0] == "call" and (len(argv) < 2 or argv[1].lower() != binding["bridge_contract"]):
            fail("cast call artifact must target the reviewed Bridge contract")
    else:
        fail("artifact tool must be dfx or cast")
    joined = " ".join(argv).lower()
    if any(marker in joined for marker in ("localhost", "127.0.0.1", "anvil", "pocket", "mock-external")):
        fail("artifact command references a local or test-double backend")
    if any(marker in joined for marker in ("private-key", "password", "authorization", "api-key")):
        fail("artifact command may expose credential material")
    if any(item.startswith(("http://", "https://")) for item in argv):
        fail("artifact command must receive RPC endpoints outside recorded argv")


def reject_secret_material(value: Any, context: str = "raw artifact") -> None:
    if isinstance(value, dict):
        for key, nested in value.items():
            lowered = str(key).lower().replace("_", "-")
            if any(marker in lowered for marker in ("private-key", "password", "authorization", "api-key", "secret", "seed")):
                fail(f"{context} contains a secret-like field")
            reject_secret_material(nested, context)
    elif isinstance(value, list):
        for nested in value:
            reject_secret_material(nested, context)
    elif isinstance(value, str) and value.startswith(("http://", "https://")):
        credential_free_https_url(value)


def validate_transport(artifact: dict[str, Any], binding: dict[str, Any]) -> None:
    transport = artifact["transport"]
    if artifact["kind"] == "fault":
        if transport is not None:
            fail("fault artifact must not contain provider transport")
        return
    if artifact["tool"] == "dfx":
        if transport is not None:
            fail("dfx artifact must not contain Base provider transport")
        return
    if not isinstance(transport, dict):
        fail("cast artifact is missing reviewed provider transport")
    exact_keys(
        transport,
        {"provider_index", "rpc_url_sha256", "observed_chain_id", "method", "params"},
        "cast artifact transport",
    )
    index = transport["provider_index"]
    if not isinstance(index, int) or isinstance(index, bool) or not 0 <= index < 3:
        fail("cast artifact provider index is invalid")
    if transport["rpc_url_sha256"] != binding["rpc_endpoints"][index]["url_sha256"]:
        fail("cast artifact endpoint is not bound to the reviewed provider index")
    if transport["observed_chain_id"] != BASE_SEPOLIA_CHAIN_ID:
        fail("cast artifact endpoint returned the wrong chain ID")
    if transport["method"] != artifact["argv"][0] or transport["params"] != artifact["argv"][1:]:
        fail("cast artifact method/params are not bound to recorded argv")


def validate_raw_artifacts(evidence: dict[str, Any], binding: dict[str, Any], root: Path) -> None:
    covered: set[str] = set()
    seen_paths: set[str] = set()
    request_records: list[list[str]] = []
    response_records: list[str] = []
    fault_claim: dict[str, Any] | None = None
    decision_events: list[dict[str, Any]] = []
    root = root.resolve()
    for reference in evidence["artifacts"]:
        relative = reference["path"]
        if relative in seen_paths:
            fail("the same raw artifact cannot satisfy multiple references")
        seen_paths.add(relative)
        path = (root / relative).resolve()
        if not path.is_relative_to(root):
            fail("raw artifact resolves outside the evidence bundle")
        try:
            payload = path.read_bytes()
        except OSError as error:
            fail(f"raw artifact is missing: {relative}: {error}")
        digest = hashlib.sha256(payload).hexdigest()
        if digest != reference["sha256"]:
            fail(f"raw artifact digest mismatch: {relative}")
        artifact = load_object(path)
        exact_keys(
            artifact,
            {"schema_version", "scenario", "kind", "captured_at", "tool", "argv", "exit_code", "stdout_sha256", "stdout", "parsed", "transport"},
            "raw artifact",
        )
        if artifact["schema_version"] != 1 or artifact["scenario"] != evidence["scenario"] or artifact["kind"] != reference["kind"]:
            fail("raw artifact binding does not match its scenario reference")
        valid_timestamp(artifact["captured_at"], "raw artifact captured_at")
        if artifact["exit_code"] != 0 or not isinstance(artifact["stdout"], str):
            fail("raw artifact command did not complete successfully")
        if hashlib.sha256(artifact["stdout"].encode()).hexdigest() != artifact["stdout_sha256"]:
            fail("raw artifact stdout digest mismatch")
        try:
            parsed = json.loads(artifact["stdout"])
        except json.JSONDecodeError as error:
            fail(f"raw artifact stdout is not JSON: {error}")
        if parsed != artifact["parsed"]:
            fail("raw artifact parsed value is not its exact stdout")
        reject_secret_material(parsed)
        if not isinstance(artifact["argv"], list) or any(not isinstance(item, str) for item in artifact["argv"]):
            fail("raw artifact argv is invalid")
        validate_capture_command(artifact["kind"], artifact["tool"], artifact["argv"], binding)
        validate_transport(artifact, binding)
        if artifact["kind"] == "fault":
            exact_keys(parsed, {"schema_version", "rehearsal_id", "scenario", "run_reference", "configured_provider_count", "required_threshold", "failed_provider_count", "failed_provider_indices", "provider_url_digests", "failure_rule", "started_at", "completed_at", "restored_provider_indices", "injector_output_digest", "request_config_digest", "decision_sequence", "decision_timestamp_ns", "decision_digest"}, "fault injection artifact")
            expected = evidence["details"]
            expected_failed = list(range(expected["injected_provider_failures"]))
            request_config = {
                "rehearsal_id": binding["rehearsal_id"],
                "scenario": evidence["scenario"],
                "run_reference": expected["fault_injection_reference"],
                "provider_url_digests": [item["url_sha256"] for item in binding["rpc_endpoints"]],
                "failed_provider_indices": expected_failed,
                "failure_rule": "connection-refused",
            }
            expected_request_digest = hashlib.sha256(json.dumps(request_config, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
            if (
                parsed["schema_version"] != 1
                or parsed["rehearsal_id"] != binding["rehearsal_id"]
                or parsed["scenario"] != evidence["scenario"]
                or parsed["run_reference"] != expected["fault_injection_reference"]
                or parsed["configured_provider_count"] != expected["configured_provider_count"]
                or parsed["required_threshold"] != expected["required_provider_threshold"]
                or parsed["failed_provider_count"] != expected["injected_provider_failures"]
                or parsed["failed_provider_indices"] != expected_failed
                or parsed["provider_url_digests"] != request_config["provider_url_digests"]
                or parsed["failure_rule"] != request_config["failure_rule"]
                or parsed["restored_provider_indices"] != expected_failed
                or parsed["request_config_digest"] != expected_request_digest
                or artifact["argv"] != [expected_request_digest, parsed["injector_output_digest"]]
                or parsed["decision_digest"] != hashlib.sha256(json.dumps(evidence["canister_decision"], sort_keys=True, separators=(",", ":")).encode()).hexdigest()
                or not isinstance(parsed["decision_sequence"], int) or parsed["decision_sequence"] < 0
                or not isinstance(parsed["decision_timestamp_ns"], int) or parsed["decision_timestamp_ns"] < 0
            ):
                fail("fault injection artifact does not match the reviewed rehearsal run")
            valid_timestamp(parsed["started_at"], "fault injection started_at")
            valid_timestamp(parsed["completed_at"], "fault injection completed_at")
            if (parsed["completed_at"] < parsed["started_at"]
                    or not isinstance(parsed["injector_output_digest"], str)
                    or not SHA256.fullmatch(parsed["injector_output_digest"])):
                fail("fault injection execution interval or output digest is invalid")
            decision_at = datetime.fromtimestamp(parsed["decision_timestamp_ns"] / 1_000_000_000, timezone.utc).isoformat().replace("+00:00", "Z")
            if not parsed["started_at"] <= decision_at <= parsed["completed_at"]:
                fail("Canister decision was not emitted during the reviewed fault interval")
            fault_claim = parsed
        if artifact["kind"] == "audit" and evidence["scenario"] in RPC_AUDIT_SCENARIOS:
            observed_audits = normalized_rpc_audits(parsed)
            if evidence["canister_audit"] not in observed_audits:
                fail("scenario Canister audit binding is not derived from get_audit_events")
        if artifact["kind"] == "audit" and evidence["scenario"] in RPC_DECISION_SCENARIOS:
            observed_decisions = normalized_rpc_decisions(parsed)
            decision_events.extend(observed_decisions)
            if evidence["canister_decision"] not in [event["decision"] for event in observed_decisions]:
                fail("scenario Canister decision is not derived from get_audit_events")
        request_records.append([artifact["tool"], *artifact["argv"], artifact["transport"]])
        response_records.append(artifact["stdout"])
        pointer_root = dict(parsed) if isinstance(parsed, dict) else {"value": parsed}
        pointer_root["@transport"] = artifact["transport"]
        for field, pointer in reference["bindings"].items():
            if field not in evidence["details"]:
                fail(f"artifact binds an unknown detail field: {field}")
            observed = json_pointer(pointer_root, pointer)
            if field == "bridge_canister_module_sha256":
                if isinstance(observed, str):
                    observed = observed.lower().removeprefix("0x")
                elif isinstance(observed, list) and all(isinstance(byte, int) and 0 <= byte <= 255 for byte in observed):
                    observed = bytes(observed).hex()
            if observed != evidence["details"][field]:
                fail(f"raw artifact disagrees with scenario detail: {field}")
            covered.add(field)
    if evidence["scenario"] in {"single_provider_failure", "quorum_loss"}:
        if fault_claim is None:
            fail("fault scenario lacks its execution claim")
        matching = [event for event in decision_events if event["sequence"] == fault_claim["decision_sequence"] and event["timestamp_ns"] == fault_claim["decision_timestamp_ns"] and hashlib.sha256(json.dumps(event["decision"], sort_keys=True, separators=(",", ":")).encode()).hexdigest() == fault_claim["decision_digest"]]
        if len(matching) != 1:
            fail("fault claim does not identify exactly one matching Canister audit event")
    if covered != set(evidence["details"]):
        fail(f"scenario details lack raw artifact bindings: {sorted(set(evidence['details']) - covered)}")
    request_digest = hashlib.sha256(
        json.dumps(request_records, separators=(",", ":"), ensure_ascii=False).encode()
    ).hexdigest()
    response_digest = hashlib.sha256(
        json.dumps(response_records, separators=(",", ":"), ensure_ascii=False).encode()
    ).hexdigest()
    if evidence["request_sha256"] != request_digest or evidence["response_sha256"] != response_digest:
        fail("scenario request/response digests are not derived from its raw artifacts")


def capture_artifact(
    manifest: dict[str, Any],
    raw_config: dict[str, Any],
    scenario: str,
    kind: str,
    output: Path,
    command: list[str],
    provider_index: int | None = None,
) -> None:
    if scenario not in SCENARIOS or kind not in {"bridge", "base", "ledger", "audit", "module"}:
        fail("capture scenario or artifact kind is invalid")
    if not command:
        fail("capture command is missing")
    tool = command[0]
    if tool not in {"dfx", "cast"}:
        fail("capture must invoke dfx or cast by its fixed executable name")
    argv = command[1:]
    config_binding = validate_config(raw_config)
    validate_binding(manifest["binding"])
    if config_binding != manifest["binding"]:
        fail("capture config does not match the reviewed manifest binding")
    validate_capture_command(kind, tool, argv, manifest["binding"])
    transport = None
    execution_command = list(command)
    if tool == "cast":
        if provider_index is None or not 0 <= provider_index < 3:
            fail("cast capture requires a reviewed provider index 0..2")
        forbidden_env = ("ETH_RPC_URL", "FOUNDRY_ETH_RPC_URL", "CAST_RPC_URL")
        if any(os.environ.get(name) for name in forbidden_env):
            fail("cast endpoint environment overrides are forbidden")
        if any(item == "--rpc-url" or item.startswith("--rpc-url=") for item in argv):
            fail("cast --rpc-url override is forbidden")
        if "--chain" in argv or "--json" in argv:
            fail("cast chain/json flags are supplied only by the fixed capture helper")
        rpc_url = raw_config["rpc_urls"][provider_index]
        clean_env = {key: value for key, value in os.environ.items() if key not in forbidden_env}
        chain = subprocess.run(
            ["cast", "chain-id", "--rpc-url", rpc_url],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=clean_env,
        )
        if chain.returncode != 0 or chain.stdout.strip() != str(BASE_SEPOLIA_CHAIN_ID):
            fail("reviewed Base provider did not report the Base Sepolia chain ID")
        execution_command.extend(["--rpc-url", rpc_url, "--chain", str(BASE_SEPOLIA_CHAIN_ID), "--json"])
        transport = {
            "provider_index": provider_index,
            "rpc_url_sha256": manifest["binding"]["rpc_endpoints"][provider_index]["url_sha256"],
            "observed_chain_id": BASE_SEPOLIA_CHAIN_ID,
            "method": argv[0],
            "params": argv[1:],
        }
        result = subprocess.run(execution_command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=clean_env)
    else:
        if provider_index is not None:
            fail("dfx capture must not select a Base provider")
        result = subprocess.run(execution_command, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode != 0:
        fail(f"capture command failed with status {result.returncode}: {result.stderr.strip()}")
    try:
        parsed = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"capture command did not emit JSON: {error}")
    reject_secret_material(parsed)
    artifact = {
        "schema_version": 1,
        "scenario": scenario,
        "kind": kind,
        "captured_at": now(),
        "tool": tool,
        "argv": argv,
        "exit_code": result.returncode,
        "stdout_sha256": hashlib.sha256(result.stdout.encode()).hexdigest(),
        "stdout": result.stdout,
        "parsed": parsed,
        "transport": transport,
    }
    write_json(output, artifact)


def capture_fault_artifact(
    manifest: dict[str, Any], raw_config: dict[str, Any], scenario: str, output: Path,
    run_reference: str, command: list[str],
) -> None:
    if scenario not in {"single_provider_failure", "quorum_loss"}:
        fail("fault capture is only valid for the two reviewed quorum scenarios")
    if command != ["evm-rpc-fault-injector"]:
        fail("fault capture must invoke the fixed evm-rpc-fault-injector executable")
    config_binding = validate_config(raw_config)
    validate_binding(manifest["binding"])
    if config_binding != manifest["binding"]:
        fail("capture config does not match the reviewed manifest binding")
    failed_indices = [0] if scenario == "single_provider_failure" else [0, 1]
    request_config = {
        "rehearsal_id": manifest["binding"]["rehearsal_id"],
        "scenario": scenario,
        "run_reference": run_reference,
        "provider_url_digests": [item["url_sha256"] for item in manifest["binding"]["rpc_endpoints"]],
        "failed_provider_indices": failed_indices,
        "failure_rule": "connection-refused",
    }
    request_json = json.dumps(request_config, sort_keys=True, separators=(",", ":"))
    request_digest = hashlib.sha256(request_json.encode()).hexdigest()
    started_at = now()
    result = subprocess.run(command, input=request_json, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    completed_at = now()
    if result.returncode != 0:
        fail(f"fault injector failed with status {result.returncode}: {result.stderr.strip()}")
    try:
        injector_result = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"fault injector did not emit JSON: {error}")
    exact_keys(injector_result, {"schema_version", "run_reference", "applied_provider_indices", "restored_provider_indices", "result", "decision_sequence", "decision_timestamp_ns", "canister_decision"}, "fault injector result")
    if (injector_result["schema_version"] != 1 or injector_result["run_reference"] != run_reference
            or injector_result["applied_provider_indices"] != failed_indices
            or injector_result["restored_provider_indices"] != failed_indices
            or injector_result["result"] != "completed"
            or not isinstance(injector_result["decision_sequence"], int)
            or not isinstance(injector_result["decision_timestamp_ns"], int)
            or not isinstance(injector_result["canister_decision"], dict)):
        fail("fault injector did not apply and restore the reviewed failure set")
    decision_at = datetime.fromtimestamp(injector_result["decision_timestamp_ns"] / 1_000_000_000, timezone.utc).isoformat().replace("+00:00", "Z")
    if not started_at <= decision_at <= completed_at:
        fail("fault injector returned a Canister decision outside its execution interval")
    reject_secret_material(injector_result)
    injector_digest = hashlib.sha256(result.stdout.encode()).hexdigest()
    parsed = {
        "schema_version": 1, "rehearsal_id": request_config["rehearsal_id"], "scenario": scenario,
        "run_reference": run_reference, "configured_provider_count": 3, "required_threshold": 2,
        "failed_provider_count": len(failed_indices), "failed_provider_indices": failed_indices,
        "provider_url_digests": request_config["provider_url_digests"], "failure_rule": request_config["failure_rule"],
        "started_at": started_at, "completed_at": completed_at,
        "restored_provider_indices": injector_result["restored_provider_indices"],
        "injector_output_digest": injector_digest, "request_config_digest": request_digest,
        "decision_sequence": injector_result["decision_sequence"],
        "decision_timestamp_ns": injector_result["decision_timestamp_ns"],
        "decision_digest": hashlib.sha256(json.dumps(injector_result["canister_decision"], sort_keys=True, separators=(",", ":")).encode()).hexdigest(),
    }
    stdout = json.dumps(parsed, sort_keys=True, separators=(",", ":"))
    artifact = {"schema_version": 1, "scenario": scenario, "kind": "fault", "captured_at": completed_at,
        "tool": "fault-injection-recorder", "argv": [request_digest, injector_digest], "exit_code": 0,
        "stdout_sha256": hashlib.sha256(stdout.encode()).hexdigest(), "stdout": stdout, "parsed": parsed, "transport": None}
    write_json(output, artifact)


def validate_details(scenario: str, details: dict[str, Any], binding: dict[str, Any]) -> None:
    if scenario == "preflight":
        exact_keys(
            details,
            {
                "observed_chain_id",
                "observed_evm_rpc_canister_id",
                "observed_bridge_contract",
                "base_bridge_signer",
                "canister_chain_key_signer",
                "deposits_paused",
                "withdrawals_paused",
                "cycles_balance",
                "base_sepolia_eth_balance_wei",
                "configured_rpc_url_sha256",
                "bridge_canister_module_sha256",
            },
            "preflight.details",
        )
        if details["observed_chain_id"] != BASE_SEPOLIA_CHAIN_ID:
            fail("preflight observed the wrong chain")
        if details["observed_evm_rpc_canister_id"] != OFFICIAL_EVM_RPC_CANISTER_ID:
            fail("preflight observed the wrong EVM RPC Canister")
        if details["bridge_canister_module_sha256"] != binding["bridge_canister_wasm_sha256"]:
            fail("preflight Bridge Canister module hash does not match the reviewed release")
        require_hex(details, "observed_bridge_contract", HEX_20)
        require_hex(details, "base_bridge_signer", HEX_20)
        require_hex(details, "canister_chain_key_signer", HEX_20)
        if details["observed_bridge_contract"].lower() != binding["bridge_contract"]:
            fail("preflight bridge contract does not match config")
        expected_signer = binding["expected_bridge_signer"]
        if (
            details["base_bridge_signer"].lower() != expected_signer
            or details["canister_chain_key_signer"].lower() != expected_signer
        ):
            fail("preflight signer triple does not match")
        if details["deposits_paused"] is not True or details["withdrawals_paused"] is not True:
            fail("the rehearsal must start with both Bridge directions paused")
        require_nat(details, "cycles_balance", positive=True)
        require_nat(details, "base_sepolia_eth_balance_wei", positive=True)
        digests = details["configured_rpc_url_sha256"]
        expected_digests = [entry["url_sha256"] for entry in binding["rpc_endpoints"]]
        if (
            not isinstance(digests, list)
            or any(not isinstance(item, str) or not SHA256.fullmatch(item) for item in digests)
            or digests != expected_digests
        ):
            fail("configured RPC URL digests do not match the rehearsal config")
        return

    if scenario == "deposit_mint":
        exact_keys(
            details,
            {"deposit_id", "ledger_block_index", "mint_transaction_hash", "safe_block_number", "safe_block_hash"},
            "deposit_mint.details",
        )
        require_hex(details, "deposit_id", HEX_32)
        require_hex(details, "mint_transaction_hash", HEX_32)
        require_hex(details, "safe_block_hash", HEX_32)
        require_nat(details, "ledger_block_index")
        require_nat(details, "safe_block_number", positive=True)
        return

    if scenario == "withdrawal_release":
        exact_keys(
            details,
            {"withdrawal_id", "request_transaction_hash", "acknowledge_transaction_hash", "ledger_block_index", "request_safe_block_number", "request_safe_block_hash", "acknowledgement_safe_block_number", "acknowledgement_safe_block_hash"},
            "withdrawal_release.details",
        )
        for field in ("withdrawal_id", "request_transaction_hash", "acknowledge_transaction_hash", "request_safe_block_hash", "acknowledgement_safe_block_hash"):
            require_hex(details, field, HEX_32)
        require_nat(details, "ledger_block_index")
        require_nat(details, "request_safe_block_number", positive=True)
        require_nat(details, "acknowledgement_safe_block_number", positive=True)
        return

    if scenario == "bad_fee_refund":
        exact_keys(
            details,
            {"withdrawal_id", "request_transaction_hash", "cancel_release_transaction_hash", "refund_transaction_hash", "old_ledger_fee", "new_ledger_fee", "amount_out_after_fee", "minimum_amount_out", "safe_block_number", "safe_block_hash"},
            "bad_fee_refund.details",
        )
        for field in ("withdrawal_id", "request_transaction_hash", "cancel_release_transaction_hash", "refund_transaction_hash", "safe_block_hash"):
            require_hex(details, field, HEX_32)
        old_fee = require_nat(details, "old_ledger_fee")
        new_fee = require_nat(details, "new_ledger_fee")
        amount_out = require_nat(details, "amount_out_after_fee")
        minimum = require_nat(details, "minimum_amount_out", positive=True)
        require_nat(details, "safe_block_number", positive=True)
        if old_fee == new_fee:
            fail("bad-fee scenario must observe an actual fee change")
        if amount_out >= minimum:
            fail("bad-fee refund scenario must fall below the requested minimum")
        return

    if scenario == "canonical_receipt":
        exact_keys(
            details,
            {"transaction_hash", "receipt_block_number", "receipt_block_hash", "canonical_block_hash", "confirmed_head_block_number"},
            "canonical_receipt.details",
        )
        for field in ("transaction_hash", "receipt_block_hash", "canonical_block_hash"):
            require_hex(details, field, HEX_32)
        receipt = require_nat(details, "receipt_block_number", positive=True)
        confirmed_head = require_nat(details, "confirmed_head_block_number", positive=True)
        if details["receipt_block_hash"].lower() != details["canonical_block_hash"].lower():
            fail("receipt block hash is not canonical")
        if confirmed_head < receipt:
            fail("receipt block has not reached the Safe head")
        return

    if scenario == "single_provider_failure":
        exact_keys(details, {"configured_provider_count", "required_provider_threshold", "injected_provider_failures", "fault_injection_reference", "threshold_satisfied", "bridge_operation_continued"}, "single_provider_failure.details")
        configured = require_nat(details, "configured_provider_count", positive=True)
        required = require_nat(details, "required_provider_threshold", positive=True)
        injected = require_nat(details, "injected_provider_failures", positive=True)
        if (
            configured != 3
            or required != 2
            or injected != 1
            or not isinstance(details["fault_injection_reference"], str)
            or not details["fault_injection_reference"]
            or details["threshold_satisfied"] is not True
            or details["bridge_operation_continued"] is not True
        ):
            fail("single-provider failure must demonstrate threshold-certified continuation")
        return

    if scenario == "quorum_loss":
        exact_keys(details, {"configured_provider_count", "required_provider_threshold", "injected_provider_failures", "fault_injection_reference", "threshold_satisfied", "fail_closed", "stop_reason", "ledger_call_performed"}, "quorum_loss.details")
        configured = require_nat(details, "configured_provider_count", positive=True)
        required = require_nat(details, "required_provider_threshold", positive=True)
        injected = require_nat(details, "injected_provider_failures", positive=True)
        if (
            configured != 3
            or required != 2
            or injected < 2
            or not isinstance(details["fault_injection_reference"], str)
            or not details["fault_injection_reference"]
            or details["threshold_satisfied"] is not False
            or details["fail_closed"] is not True
            or details["stop_reason"] not in ("RpcInconsistent", "RpcUnavailable")
            or details["ledger_call_performed"] is not False
        ):
            fail("quorum loss must fail closed before a Ledger call")
        return

    if scenario == "nonce_known":
        exact_keys(details, {"nonce_too_low_observed", "local_transaction_hash", "provider_agreement", "resulting_state"}, "nonce_known.details")
        require_hex(details, "local_transaction_hash", HEX_32)
        if details["nonce_too_low_observed"] is not True or details["provider_agreement"] != 2 or details["resulting_state"] != "Submitted":
            fail("known nonce scenario must recover only a provider-agreed local transaction")
        return

    if scenario == "nonce_conflict":
        exact_keys(details, {"nonce_too_low_observed", "local_transaction_hash", "resulting_stop_reason", "deposits_paused", "automatically_resigned"}, "nonce_conflict.details")
        if details != {"nonce_too_low_observed": True, "local_transaction_hash": None, "resulting_stop_reason": "NonceConflict", "deposits_paused": True, "automatically_resigned": False}:
            fail("unknown nonce conflict must pause deposits without re-signing")
        return

    if scenario == "final_pause":
        exact_keys(
            details,
            {"base_deposits_paused", "base_withdrawals_paused", "canister_deposits_paused", "safe_block_number", "safe_block_hash"},
            "final_pause.details",
        )
        require_hex(details, "safe_block_hash", HEX_32)
        require_nat(details, "safe_block_number", positive=True)
        if (
            details["base_deposits_paused"] is not True
            or details["base_withdrawals_paused"] is not True
            or details["canister_deposits_paused"] is not True
        ):
            fail("the rehearsal must end with Base and Canister asset intake paused")
        return

    fail(f"unsupported scenario: {scenario}")


def derive_state(scenarios: dict[str, Any]) -> tuple[str, bool]:
    completed = {name for name, evidence in scenarios.items() if evidence is not None}
    if "preflight" not in completed:
        return "AWAITING_PREFLIGHT", False
    flow = {"deposit_mint", "withdrawal_release", "bad_fee_refund", "canonical_receipt"}
    faults = {"single_provider_failure", "quorum_loss", "nonce_known", "nonce_conflict"}
    if not flow.issubset(completed):
        return "READY_FOR_ASSET_FLOWS", False
    if not faults.issubset(completed):
        return "READY_FOR_FAILURE_SCENARIOS", False
    if "final_pause" not in completed:
        return "READY_FOR_FINAL_PAUSE", False
    return "COMPLETE", True


def record(
    manifest: dict[str, Any],
    evidence: dict[str, Any],
    scenario: str,
    artifact_root: Path | None = None,
) -> None:
    validate_manifest_envelope(manifest)
    if scenario not in SCENARIOS:
        fail(f"unknown scenario: {scenario}")
    scenarios = manifest.get("scenarios")
    if not isinstance(scenarios, dict) or set(scenarios) != set(SCENARIOS):
        fail("manifest scenario set is invalid")
    if not isinstance(manifest.get("binding"), dict):
        fail("manifest binding is missing")
    validate_binding(manifest["binding"])
    if scenario != "preflight" and scenarios["preflight"] is None:
        fail("preflight evidence must be recorded first")
    validate_common(evidence, manifest["binding"], scenario)
    validate_details(scenario, evidence["details"], manifest["binding"])
    if artifact_root is not None:
        validate_raw_artifacts(evidence, manifest["binding"], artifact_root)
    current = scenarios[scenario]
    if current is not None and current != evidence:
        fail("conflicting evidence already exists for this scenario")
    scenarios[scenario] = evidence
    manifest["state"], manifest["complete"] = derive_state(scenarios)
    if manifest["complete"] and artifact_root is None:
        manifest["state"] = "AWAITING_RAW_ARTIFACT_VERIFICATION"
        manifest["complete"] = False
    manifest["updated_at"] = now()


def verify_manifest(manifest: dict[str, Any], artifact_root: Path | None = None) -> None:
    validate_manifest_envelope(manifest)
    binding = manifest.get("binding")
    if not isinstance(binding, dict):
        fail("manifest binding is missing")
    validate_binding(binding)
    scenarios = manifest.get("scenarios")
    if not isinstance(scenarios, dict) or set(scenarios) != set(SCENARIOS):
        fail("manifest scenario set is invalid")
    for scenario, evidence in scenarios.items():
        if evidence is not None:
            if not isinstance(evidence, dict):
                fail(f"{scenario} evidence must be an object")
            validate_common(evidence, binding, scenario)
            validate_details(scenario, evidence["details"], binding)
            if artifact_root is not None:
                validate_raw_artifacts(evidence, binding, artifact_root)
    state, complete = derive_state(scenarios)
    if complete and artifact_root is None:
        fail("COMPLETE rehearsal verification requires the raw artifact directory")
    if manifest.get("state") != state or manifest.get("complete") is not complete:
        fail("manifest state does not match its evidence")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate = subparsers.add_parser("validate-config")
    validate.add_argument("config", type=Path)
    initialize = subparsers.add_parser("init")
    initialize.add_argument("config", type=Path)
    initialize.add_argument("manifest", type=Path)
    initialize.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    add = subparsers.add_parser("record")
    add.add_argument("manifest", type=Path)
    add.add_argument("scenario", choices=SCENARIOS)
    add.add_argument("evidence", type=Path)
    capture = subparsers.add_parser("capture-artifact")
    capture.add_argument("manifest", type=Path)
    capture.add_argument("config", type=Path)
    capture.add_argument("scenario", choices=SCENARIOS)
    capture.add_argument("kind", choices=("bridge", "base", "ledger", "audit", "module"))
    capture.add_argument("output", type=Path)
    capture.add_argument("provider_selection", help="0..2 for Base capture; 'none' for dfx")
    capture.add_argument("capture_argv", nargs=argparse.REMAINDER)
    fault = subparsers.add_parser("capture-fault")
    fault.add_argument("manifest", type=Path)
    fault.add_argument("config", type=Path)
    fault.add_argument("scenario", choices=("single_provider_failure", "quorum_loss"))
    fault.add_argument("output", type=Path)
    fault.add_argument("run_reference")
    fault.add_argument("capture_argv", nargs=argparse.REMAINDER)
    verify = subparsers.add_parser("verify")
    verify.add_argument("manifest", type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        if args.command == "validate-config":
            sanitized = validate_config(load_object(args.config))
            print(json.dumps(sanitized, indent=2, sort_keys=True))
        elif args.command == "init":
            if args.manifest.exists():
                fail("refusing to overwrite an existing rehearsal manifest")
            config = validate_config(load_object(args.config))
            write_json(args.manifest, initial_manifest(config, args.repo_root.resolve()))
            print(f"initialized {args.manifest}")
        elif args.command == "record":
            manifest = load_object(args.manifest)
            record(
                manifest,
                load_object(args.evidence),
                args.scenario,
                args.manifest.resolve().parent,
            )
            write_json(args.manifest, manifest)
            print(f"recorded {args.scenario}; state={manifest['state']}")
        elif args.command == "capture-artifact":
            command = args.capture_argv[1:] if args.capture_argv[:1] == ["--"] else args.capture_argv
            if args.provider_selection == "none":
                provider_index = None
            elif args.provider_selection in {"0", "1", "2"}:
                provider_index = int(args.provider_selection)
            else:
                fail("provider selection must be 0..2 or 'none'")
            manifest = load_object(args.manifest)
            validate_manifest_envelope(manifest)
            capture_artifact(
                manifest,
                load_object(args.config),
                args.scenario,
                args.kind,
                args.output,
                command,
                provider_index,
            )
            print(f"captured {args.kind} artifact at {args.output}")
        elif args.command == "capture-fault":
            command = args.capture_argv[1:] if args.capture_argv[:1] == ["--"] else args.capture_argv
            manifest = load_object(args.manifest)
            validate_manifest_envelope(manifest)
            capture_fault_artifact(manifest, load_object(args.config), args.scenario, args.output, args.run_reference, command)
            print(f"captured fault artifact at {args.output}")
        elif args.command == "verify":
            manifest = load_object(args.manifest)
            verify_manifest(manifest, args.manifest.resolve().parent)
            print(f"valid rehearsal manifest; state={manifest['state']}")
        return 0
    except (InvalidEvidence, subprocess.CalledProcessError) as error:
        print(f"evm-rpc-rehearsal: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
