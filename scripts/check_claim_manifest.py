#!/usr/bin/env python3
"""Validate the unified typed claim manifest and compute evidence strength."""

from __future__ import annotations

import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

from claim_manifest import (
    REQUIRED_CLAIM_POLICY,
    REQUIRED_CLAIM_IDS,
    ClaimManifest,
    ConditionalLivenessProperty,
    conditional_liveness_check_source,
    lean_contract_check_source,
    parse_conditional_liveness_manifest,
    parse_claim_manifest,
)
from halmos_obligations import parse_halmos_obligations
from proof_fingerprint import source_fingerprint
from source_resolution import is_inside_source_roots, source_path
from smt_obligations import parse_smt_obligations
from verus_manifest import parse_verus_manifest
from check_transition_manifest import strip_comments_and_strings
from rust_canonical_calls import rust_body

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "verification" / "claims.tsv"
CONDITIONAL_LIVENESS = ROOT / "verification" / "conditional-liveness.tsv"
REPORT = Path(
    os.environ.get(
        "BRIDGE_CLAIM_REPORT",
        str(ROOT / "verification" / "output" / "claim-report.json"),
    )
)
CLAIM_REPORT_SCHEMA = 7
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
REQUIRED_SCALAR_CALLS = (
    "deadlineAccepts(",
    "epochMatches(",
    "depositAvailable(",
    "feeWithinBounds(",
    "mintAmountWithinLimit(",
    "windowExpired(",
    "MintAccounting.tryConsumeWindow(",
    "mintEffectAmounts(",
)
ALLOWED_LEAN_AXIOMS = frozenset({"propext", "Classical.choice", "Quot.sound"})


def require_guard_dominance(
    source: str, function: str, guard: str, effects: tuple[str, ...]
) -> None:
    body = rust_body(strip_comments_and_strings(source), function)
    if body.count(guard) != 1:
        raise ValueError(f"{function} must contain one lifecycle guard")
    guard_position = body.index(guard)
    effect_positions = [body.index(effect) for effect in effects if effect in body]
    if not effect_positions or guard_position > min(effect_positions):
        raise ValueError(f"{function} lifecycle guard must dominate state and external effects")


def check_operational_config_guard_dominance() -> None:
    specs = {
        "canister/bridge-canister/src/lib.rs": {
            "request_deposit": ("require_asset_operations_for_deposit()", ("msg_caller()", "STORE.with", "InFlightGuard::acquire")),
            "request_deposit_refund": ("require_asset_operations_for_refund()", ("msg_caller()", "STORE.with", "InFlightGuard::acquire")),
            "notify_withdrawal": ("require_asset_operations_for_withdrawal_notification()", ("msg_caller()", "STORE.with", "InFlightGuard::acquire")),
            "continue_deposit": ("require_asset_operations_for_settlement()", ("msg_caller()", "STORE.with", "InFlightGuard::acquire")),
            "continue_withdrawal": ("require_asset_operations_for_settlement()", ("msg_caller()", "STORE.with", "InFlightGuard::acquire")),
            "request_fee_payout": ("require_asset_operations_for_fee_payout()", ("msg_caller()", "InFlightGuard::acquire", "admin::request_fee_payout")),
            "continue_fee_payout": ("require_asset_operations_for_settlement()", ("msg_caller()", "STORE.with", "InFlightGuard::acquire")),
            "icrc21_canister_call_consent_message": ("admit_consent_request()", ("ledger::KINIC_LEDGER_FEE", "consent::consent_message")),
            "admit_consent_request": ("asset_operations_are_available()", ("STORE.with", "has_liability_cycle_budget")),
            "prepare_base_governance_action": ("base_governance::require_operational_config_sealed()", ("InFlightGuard::acquire", "msg_caller()", "base_governance::prepare")),
            "refresh_activation_attestation": ("base_governance::require_operational_config_sealed()", ("msg_caller()", "activation_attestation()", "admit_control_plane_external_call")),
            "confirm_base_governance_transaction": ("base_governance::require_operational_config_sealed()", ("msg_caller()", "notification_failure_cooldown_active", "admit_control_plane_external_call")),
            "prepare_base_governance_replacement": ("base_governance::require_operational_config_sealed()", ("InFlightGuard::acquire", "msg_caller()", "prepare_replacement")),
            "prepare_next_emergency_base_action": ("base_governance::require_operational_config_sealed()", ("InFlightGuard::acquire", "msg_caller()", "prepare_next_emergency")),
            "schedule_activation": ("base_governance::require_operational_config_sealed()", ("InFlightGuard::acquire", "msg_caller()", "base_governance::prepare")),
            "execute_activation": ("base_governance::require_operational_config_sealed()", ("InFlightGuard::acquire", "msg_caller()", "base_governance::prepare")),
        },
        "canister/bridge-canister/src/base_governance.rs": {
            "refresh_activation_attestation": ("require_operational_config_sealed()", ("config()", "activation_preflight")),
            "prepare": ("require_operational_config_sealed()", ("require_action_authorization", "config()", "STORE.with")),
            "prepare_replacement": ("require_operational_config_sealed()", ("require_governance_or_pause", "pending_transaction", "sign_prepared")),
            "confirm": ("require_operational_config_sealed()", ("require_confirmation_caller", "pending_transaction", "evm_rpc::")),
            "prepare_next_emergency": ("require_operational_config_sealed()", ("require_governance_or_pause", "STORE.with", "prepare(")),
        },
        "canister/bridge-canister/src/scheduler.rs": {
            "arm_funding_recovery": ("asset_operations_allowed()", ("STORE.with", "set_timer")),
            "recover_one_funding_attempt": ("asset_operations_allowed()", ("STORE.with", "msg_caller()")),
            "arm": ("asset_operations_allowed()", ("STORE.with", "next_settlement_wakeup_ns")),
            "dispatch_due": ("asset_operations_allowed()", ("STORE.with", "claim_due_settlement_job")),
            "run_claimed": ("asset_operations_allowed()", ("run_claimed_inner",)),
            "run_claimed_fee_payout": ("asset_operations_allowed()", ("SettlementLease::new", "settlement_retry_interval_seconds")),
        },
    }
    for relative_path, functions in specs.items():
        source = (ROOT / relative_path).read_text(encoding="utf-8")
        for function, (guard, effects) in functions.items():
            try:
                require_guard_dominance(source, function, guard, effects)
            except ValueError as error:
                raise ValueError(f"{relative_path}#{error}") from error


def items(value: str) -> list[str]:
    return [] if value == "-" else value.split(";")


def abstract_evidence_status(value: str) -> str:
    return "proved" if items(value) else "not-applicable"


EVIDENCE_STRENGTH = {
    "unlinked": 0,
    "abstract-proved": 1,
    "production-linked": 2,
    "implementation-proved": 3,
}


def required_strength_met(actual: str, required: str) -> bool:
    return EVIDENCE_STRENGTH[actual] >= EVIDENCE_STRENGTH[required]


def checked_link(value: str) -> tuple[Path, str]:
    if value.count("#") != 1:
        raise ValueError(f"invalid source link: {value}")
    path_text, symbol = value.split("#")
    if not IDENTIFIER.fullmatch(symbol):
        raise ValueError(f"invalid source symbol: {value}")
    path = source_path(path_text).resolve()
    if not is_inside_source_roots(path) or not path.is_file():
        raise ValueError(f"missing source link target: {value}")
    if re.search(rf"\b{re.escape(symbol)}\b", path.read_text(encoding="utf-8")) is None:
        raise ValueError(f"missing registered source symbol: {value}")
    return path, symbol


def strip_solidity_comments_and_strings(source: str) -> str:
    """Preserve offsets while removing Solidity comments and quoted strings."""
    result = list(source)
    index = 0
    state = "code"
    quote = ""
    while index < len(source):
        pair = source[index : index + 2]
        char = source[index]
        if state == "code":
            if pair == "//":
                result[index] = result[index + 1] = " "
                index += 2
                state = "line"
                continue
            if pair == "/*":
                result[index] = result[index + 1] = " "
                index += 2
                state = "block"
                continue
            if char in {'"', "'"}:
                quote = char
                result[index] = " "
                state = "string"
        elif state == "line":
            if char == "\n":
                state = "code"
            else:
                result[index] = " "
        elif state == "block":
            result[index] = " "
            if pair == "*/":
                result[index + 1] = " "
                index += 2
                state = "code"
                continue
        else:
            result[index] = " "
            if char == "\\" and index + 1 < len(source):
                result[index + 1] = " "
                index += 2
                continue
            if char == quote:
                state = "code"
        index += 1
    if state in {"block", "string"}:
        raise ValueError(f"unterminated Solidity {state}")
    return "".join(result)


def solidity_function_body(source: str, name: str) -> str:
    cleaned = strip_solidity_comments_and_strings(source)
    marker = re.search(rf"\bfunction\s+{re.escape(name)}\s*\(", cleaned)
    if marker is None:
        raise ValueError(f"missing Solidity function: {name}")
    brace = cleaned.find("{", marker.end())
    semicolon = cleaned.find(";", marker.end())
    if brace < 0 or (semicolon >= 0 and semicolon < brace):
        raise ValueError(f"Solidity function has no body: {name}")
    depth = 0
    for index in range(brace, len(cleaned)):
        if cleaned[index] == "{":
            depth += 1
        elif cleaned[index] == "}":
            depth -= 1
            if depth == 0:
                return cleaned[brace : index + 1]
    raise ValueError(f"Solidity function body is not balanced: {name}")


def checked_solidity_function_link(value: str) -> tuple[Path, str]:
    if value.count("#") != 1:
        raise ValueError(f"invalid Solidity source link: {value}")
    path_text, signature = value.split("#")
    match = re.fullmatch(
        r"([A-Za-z_][A-Za-z0-9_]*)\.([A-Za-z_][A-Za-z0-9_]*)\((.*)\)",
        signature,
    )
    if match is None:
        raise ValueError(f"invalid Solidity source symbol: {value}")
    _, symbol, _ = match.groups()
    path = source_path(path_text).resolve()
    if not is_inside_source_roots(path) or path.suffix != ".sol" or not path.is_file():
        raise ValueError(f"missing Solidity source link target: {value}")
    solidity_function_body(path.read_text(encoding="utf-8"), symbol)
    return path, signature


def require_exact_claim_coverage(
    label: str,
    declared: dict[str, set[str]],
    referenced: dict[str, set[str]],
) -> None:
    if set(declared) != set(referenced):
        raise ValueError(
            f"{label} obligation coverage differs: "
            f"missing={sorted(set(declared) - set(referenced))} "
            f"extra={sorted(set(referenced) - set(declared))}"
        )
    mismatches = {
        obligation_id: {
            "missing": sorted(declared[obligation_id] - referenced[obligation_id]),
            "extra": sorted(referenced[obligation_id] - declared[obligation_id]),
        }
        for obligation_id in declared
        if declared[obligation_id] != referenced[obligation_id]
    }
    if mismatches:
        raise ValueError(f"{label} obligation claim coverage differs: {mismatches}")


def require_exact_smt_claim_coverage(
    declared: dict[str, set[str]], referenced: dict[str, set[str]]
) -> None:
    require_exact_claim_coverage("SMT", declared, referenced)


def require_unique_smt_obligations(claim_id: str, obligation_ids: list[str]) -> None:
    if len(obligation_ids) != len(set(obligation_ids)):
        raise ValueError(f"duplicate SMT obligations for {claim_id}")


def require_unique_items(label: str, claim_id: str, values: list[str]) -> None:
    if len(values) != len(set(values)):
        raise ValueError(f"duplicate {label} for {claim_id}")


def require_exact_implementation_basis(
    claim_id: str,
    basis: list[str],
    verus_proofs: list[str],
    halmos_ids: list[str],
    verus_rows: dict[str, list[object]],
    halmos_obligations: dict[str, object],
) -> set[str]:
    required = {
        f"verus:{proof}"
        for proof in verus_proofs
        if any(
            registration.production_bound
            for registration in verus_rows[proof]
        )
    }
    actual = set(basis)
    if actual != required:
        raise ValueError(
            f"implementation basis differs for {claim_id}: "
            f"missing={sorted(required - actual)} extra={sorted(actual - required)}"
        )
    return required


def uncovered_verus_obligations(
    claim_id: str,
    verus_proofs: list[str],
    verus_rows: dict[str, list[object]],
) -> set[str]:
    """Return claim obligations without a complete production binding."""
    kernels = {
        registration.kernel: registration
        for registrations in verus_rows.values()
        for registration in registrations
    }
    referenced = set(verus_proofs)
    uncovered: set[str] = set()
    for proof in verus_proofs:
        registrations = verus_rows[proof]
        if any(registration.production_bound for registration in registrations):
            continue
        derived_is_covered = any(
            registration.kind == "derived"
            and bool(registration.binding)
            and all(
                (dependency := kernels.get(kernel)) is not None
                and dependency.kind in {"executable", "shared-expression"}
                and dependency.production_bound
                and dependency.proof in referenced
                and claim_id in dependency.claim_ids
                for kernel in registration.binding
            )
            for registration in registrations
        )
        if not derived_is_covered:
            uncovered.add(proof)
    return uncovered


def missing_scalar_calls(function_body: str) -> list[str]:
    return [call for call in REQUIRED_SCALAR_CALLS if call not in function_body]


def check_solidity_wrapper_refinement() -> None:
    policy = (
        ROOT / "contracts" / "src" / "libraries" / "MintAuthorizationPolicy.sol"
    ).read_text(encoding="utf-8")
    bridge = (ROOT / "contracts" / "src" / "Bridge.sol").read_text(encoding="utf-8")
    evaluate = solidity_function_body(policy, "evaluateMint")
    missing = missing_scalar_calls(evaluate)
    if missing:
        raise ValueError(f"Mint struct wrapper bypasses scalar kernels: {missing}")
    mint_wrapper = solidity_function_body(bridge, "mintDepositWithAuthorization")
    commit = solidity_function_body(bridge, "_commitAuthorizedMint")
    wrapper_requirements = (
        "MintAuthorizationPolicy.evaluateMint(",
        "_commitAuthorizedMint(",
    )
    missing = [value for value in wrapper_requirements if value not in mint_wrapper]
    if (
        missing
        or mint_wrapper.count("MintAuthorizationPolicy.evaluateMint(") != 1
        or mint_wrapper.count("_commitAuthorizedMint(") != 1
    ):
        raise ValueError(f"Bridge mint wrapper does not apply one exact transition: {missing}")
    required_effect_applications = (
        "= effects.processedAfter;",
        "= effects.windowStartedAtAfter;",
        "= effects.windowConsumedAfter;",
        "effects.supplyIncrease",
        "effects.eventGrossAmount",
        "effects.eventServiceFee",
        "effects.eventMintedAmount",
    )
    missing = [value for value in required_effect_applications if value not in commit]
    if missing:
        raise ValueError(f"Bridge mint commit does not apply one exact transition: {missing}")


def validate_lean_axiom_output(output: str, expected: int, label: str) -> None:
    printed = re.findall(r"depends on axioms: \[([^\]]*)\]", output)
    no_axioms = output.count("does not depend on any axioms")
    if len(printed) + no_axioms != expected:
        raise ValueError(f"Lean did not report an axiom dependency set for every {label}")
    for dependencies in printed:
        actual = {name.strip() for name in dependencies.split(",") if name.strip()}
        forbidden = actual - ALLOWED_LEAN_AXIOMS
        if forbidden:
            raise ValueError(
                f"Lean {label} depends on project-local axioms: {sorted(forbidden)}"
            )


def run_lean_evidence_check(
    source: str, build_target: str, expected: int, label: str
) -> None:
    build = subprocess.run(
        ["lake", "build", build_target],
        cwd=ROOT / "verification" / "lean",
        capture_output=True,
        text=True,
        check=False,
    )
    if build.returncode != 0:
        raise ValueError(f"Lean {label} build failed:\n{build.stdout}{build.stderr}")
    with tempfile.NamedTemporaryFile(mode="w", suffix=".lean", encoding="utf-8") as check:
        check.write(source)
        check.flush()
        result = subprocess.run(
            ["lake", "env", "lean", check.name],
            cwd=ROOT / "verification" / "lean",
            capture_output=True,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        raise ValueError(f"Lean {label} check failed:\n{result.stdout}{result.stderr}")
    validate_lean_axiom_output(result.stdout, expected, label)


def check_lean_claim_contracts(manifest_text: str) -> None:
    manifest = parse_claim_manifest(manifest_text)
    require_mandatory_claim_catalog(manifest)
    proved = sum(registration.is_proved for registration in manifest.contracts.values())
    run_lean_evidence_check(
        lean_contract_check_source(manifest),
        "BridgeSpec.ClaimContracts",
        proved,
        "claim witness",
    )


def check_conditional_liveness_theorems(
    properties: dict[str, ConditionalLivenessProperty],
) -> None:
    run_lean_evidence_check(
        conditional_liveness_check_source(properties),
        "BridgeSpec.Liveness",
        len(properties),
        "conditional liveness theorem",
    )


def require_mandatory_claim_catalog(manifest: ClaimManifest) -> None:
    actual = {row[1] for row in manifest.rows}
    if actual != REQUIRED_CLAIM_IDS:
        raise ValueError(
            "mandatory claim catalog differs: "
            f"missing={sorted(REQUIRED_CLAIM_IDS - actual)} "
            f"extra={sorted(actual - REQUIRED_CLAIM_IDS)}"
        )
    mismatches = {
        claim_id: {
            "expected": REQUIRED_CLAIM_POLICY[claim_id],
            "actual": (
                manifest.contracts[claim_id].assurance_target,
                manifest.contracts[claim_id].required_strength,
            ),
        }
        for claim_id in REQUIRED_CLAIM_IDS
        if (
            manifest.contracts[claim_id].assurance_target,
            manifest.contracts[claim_id].required_strength,
        )
        != REQUIRED_CLAIM_POLICY[claim_id]
    }
    if mismatches:
        raise ValueError(f"mandatory claim policy differs: {mismatches}")


def build_claim_report() -> dict[str, object]:
    check_solidity_wrapper_refinement()
    manifest_text = MANIFEST.read_text(encoding="utf-8")
    manifest = parse_claim_manifest(manifest_text)
    conditional_liveness = parse_conditional_liveness_manifest(
        CONDITIONAL_LIVENESS.read_text(encoding="utf-8")
    )
    require_mandatory_claim_catalog(manifest)
    smt_obligations_by_id = parse_smt_obligations(
        (ROOT / "verification" / "smt" / "obligations.tsv").read_text(encoding="utf-8")
    )
    halmos_obligations_by_id = parse_halmos_obligations(
        (ROOT / "verification" / "halmos" / "obligations.tsv").read_text(encoding="utf-8")
    )
    claim_ids = {row[1] for row in manifest.rows}
    failure_rows = [
        line.split("\t")
        for line in (ROOT / "verification" / "smt" / "failure-manifest.tsv")
        .read_text(encoding="utf-8")
        .splitlines()
        if line
    ]
    failure_ids = {row[0] for row in failure_rows if len(row) == 2}
    if len(failure_ids) != len(failure_rows):
        raise ValueError("invalid or duplicate SMT failure IDs")
    for obligation in smt_obligations_by_id.values():
        for link in obligation.pass_links + obligation.production_links:
            checked_solidity_function_link(link)
        unknown_failures = set(obligation.failure_ids) - failure_ids
        if unknown_failures:
            raise ValueError(
                f"unknown SMT negative obligation for {obligation.obligation_id}: "
                f"{sorted(unknown_failures)}"
            )
        unknown_claims = set(obligation.claim_ids) - claim_ids
        if unknown_claims:
            raise ValueError(
                f"unknown SMT claim for {obligation.obligation_id}: {sorted(unknown_claims)}"
            )
    halmos_failure_rows = [
        line.split("\t")
        for line in (ROOT / "verification" / "halmos" / "failure-manifest.tsv")
        .read_text(encoding="utf-8")
        .splitlines()
        if line
    ]
    halmos_failure_ids = {row[0] for row in halmos_failure_rows if len(row) == 2}
    if len(halmos_failure_ids) != len(halmos_failure_rows):
        raise ValueError("invalid or duplicate Halmos failure IDs")
    for obligation in halmos_obligations_by_id.values():
        for link in obligation.test_links + obligation.production_links:
            checked_link(link)
        unknown_failures = set(obligation.failure_ids) - halmos_failure_ids
        if unknown_failures:
            raise ValueError(
                f"unknown Halmos negative obligation for {obligation.obligation_id}: "
                f"{sorted(unknown_failures)}"
            )
        unknown_claims = set(obligation.claim_ids) - claim_ids
        if unknown_claims:
            raise ValueError(
                f"unknown Halmos claim for {obligation.obligation_id}: "
                f"{sorted(unknown_claims)}"
            )
    check_lean_claim_contracts(manifest_text)
    check_conditional_liveness_theorems(conditional_liveness)
    lean_source = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted((ROOT / "verification" / "lean" / "BridgeSpec").glob("*.lean"))
    )
    lean_theorems = set(
        re.findall(r"^theorem ([A-Za-z_][A-Za-z0-9_]*)\b", lean_source, re.MULTILINE)
    )
    verus_source = (ROOT / "verification" / "verus" / "pass.rs").read_text(encoding="utf-8")
    verus_manifest = parse_verus_manifest(
        (ROOT / "verification" / "verus" / "manifest.tsv").read_text(encoding="utf-8")
    )
    verus_rows: dict[str, list[object]] = {}
    for obligation in verus_manifest.values():
        verus_rows.setdefault(obligation.proof, []).append(obligation)
        unknown_claims = set(obligation.claim_ids) - claim_ids
        if unknown_claims:
            raise ValueError(
                f"unknown Verus claim for {obligation.obligation_id}: "
                f"{sorted(unknown_claims)}"
            )
    assumption_dependencies: dict[str, set[str]] = {}
    for number, line in enumerate(
        (ROOT / "verification" / "assumptions.tsv")
        .read_text(encoding="utf-8")
        .splitlines(),
        1,
    ):
        fields = line.split("\t")
        if len(fields) != 6 or not all(fields):
            raise ValueError(f"invalid external assumption row {number}")
        assumption, _, dependent_claims, validation_links, _, _ = fields
        if assumption in assumption_dependencies:
            raise ValueError(f"duplicate external assumption: {assumption}")
        dependencies = set(dependent_claims.split(";"))
        if not dependencies:
            raise ValueError(f"external assumption has no dependent claim: {assumption}")
        for link in validation_links.split(";"):
            checked_link(link)
        assumption_dependencies[assumption] = dependencies
    assumptions = set(assumption_dependencies)
    actual_assumption_dependencies = {
        assumption: set() for assumption in assumption_dependencies
    }
    for property in conditional_liveness.values():
        unknown_assumptions = set(property.assumption_ids) - assumptions
        if unknown_assumptions:
            raise ValueError(
                f"unknown conditional liveness assumption for {property.property_id}: "
                f"{sorted(unknown_assumptions)}"
            )
        for assumption in property.assumption_ids:
            actual_assumption_dependencies[assumption].add(property.property_id)
    vector_sections = {
        line.split("\t", 1)[0]
        for line in (ROOT / "verification" / "refinement-manifest.tsv")
        .read_text(encoding="utf-8")
        .splitlines()
        if line
    }

    rows = manifest.rows

    used_verus: set[str] = set()
    referenced_verus_claims = {proof: set() for proof in verus_rows}
    referenced_smt_claims = {
        obligation_id: set() for obligation_id in smt_obligations_by_id
    }
    referenced_halmos_claims = {
        obligation_id: set() for obligation_id in halmos_obligations_by_id
    }
    results: list[dict[str, object]] = []
    for (
        kind,
        claim_id,
        abstract_theorems,
        refinement_theorems,
        trace_theorems,
        verus_obligations,
        smt_obligations,
        halmos_obligations,
        implementation_basis,
        production_links,
        transaction_tests,
        assumption_ids,
        vectors,
    ) in rows:
        if kind not in {"protocol", "mint"} or not IDENTIFIER.fullmatch(claim_id):
            raise ValueError(f"invalid typed claim: {kind}/{claim_id}")
        theorem_names = (
            items(abstract_theorems)
            + items(refinement_theorems)
            + items(trace_theorems)
        )
        missing_theorems = set(theorem_names) - lean_theorems
        if missing_theorems:
            raise ValueError(
                f"missing Lean theorem for {claim_id}: {sorted(missing_theorems)}"
            )
        obligations = items(verus_obligations)
        require_unique_items("Verus obligations", claim_id, obligations)
        unknown_verus = set(obligations) - set(verus_rows)
        if unknown_verus:
            raise ValueError(
                f"unknown Verus obligation for {claim_id}: {sorted(unknown_verus)}"
            )
        used_verus.update(obligations)
        for proof in obligations:
            registration = verus_rows[proof][0]
            if claim_id not in registration.claim_ids:
                raise ValueError(
                    f"Verus obligation does not declare claim {claim_id}: "
                    f"{registration.obligation_id}"
                )
            referenced_verus_claims[proof].add(claim_id)
        claim_smt_ids = items(smt_obligations)
        require_unique_smt_obligations(claim_id, claim_smt_ids)
        raw_smt_links = [value for value in claim_smt_ids if "#" in value or "/" in value]
        if raw_smt_links:
            raise ValueError(f"raw SMT links are forbidden for {claim_id}: {raw_smt_links}")
        unknown_smt = set(claim_smt_ids) - set(smt_obligations_by_id)
        if unknown_smt:
            raise ValueError(f"unknown SMT obligation for {claim_id}: {sorted(unknown_smt)}")
        for obligation_id in claim_smt_ids:
            obligation = smt_obligations_by_id[obligation_id]
            if claim_id not in obligation.claim_ids:
                raise ValueError(
                    f"SMT obligation does not declare claim {claim_id}: {obligation_id}"
                )
            referenced_smt_claims[obligation_id].add(claim_id)
        claim_halmos_ids = items(halmos_obligations)
        require_unique_items("Halmos obligations", claim_id, claim_halmos_ids)
        unknown_halmos = set(claim_halmos_ids) - set(halmos_obligations_by_id)
        if unknown_halmos:
            raise ValueError(
                f"unknown Halmos obligation for {claim_id}: {sorted(unknown_halmos)}"
            )
        for obligation_id in claim_halmos_ids:
            obligation = halmos_obligations_by_id[obligation_id]
            if claim_id not in obligation.claim_ids:
                raise ValueError(
                    f"Halmos obligation does not declare claim {claim_id}: {obligation_id}"
                )
            referenced_halmos_claims[obligation_id].add(claim_id)

        basis = items(implementation_basis)
        require_unique_items("implementation basis", claim_id, basis)
        required_basis = require_exact_implementation_basis(
            claim_id,
            basis,
            obligations,
            claim_halmos_ids,
            verus_rows,
            halmos_obligations_by_id,
        )
        production = [checked_link(link) for link in items(production_links)]
        tests = [checked_link(link) for link in items(transaction_tests)]
        unknown_assumptions = set(items(assumption_ids)) - assumptions
        if unknown_assumptions:
            raise ValueError(
                f"unknown assumption for {claim_id}: {sorted(unknown_assumptions)}"
            )
        for assumption in items(assumption_ids):
            actual_assumption_dependencies[assumption].add(claim_id)
        if vectors != "-" and vectors not in vector_sections:
            raise ValueError(f"unknown refinement vector section for {claim_id}: {vectors}")

        production_text = "\n".join(
            path.read_text(encoding="utf-8") for path, _ in production
        )
        uncovered_verus = uncovered_verus_obligations(
            claim_id, obligations, verus_rows
        )
        unreferenced_kernels = [
            registration.kernel
            for obligation in obligations
            for registration in verus_rows[obligation]
            if registration.production_bound
            and re.search(
                rf"\b{re.escape(registration.kernel)}\b", production_text
            )
            is None
        ]
        vector_consumer = (
            "generated-refinement-tested"
            if vectors != "-"
            else "not-applicable"
        )
        formal_strength = (
            "implementation-proved"
            if required_basis and not uncovered_verus
            else "abstract-proved"
        )
        registration = manifest.contracts[claim_id]
        implementation_strength = (
            "unlinked"
            if not registration.is_proved
            else formal_strength
            if formal_strength == "implementation-proved"
            else "production-linked"
            if production and tests
            else "abstract-proved"
        )
        typed_basis = [
            {"kind": "production-symbol", "id": link}
            for link in items(production_links)
        ]
        typed_basis.extend(
            {"kind": "transaction-test", "id": link}
            for link in items(transaction_tests)
        )
        typed_basis.extend(
            {"kind": "formal-verus", "id": value.removeprefix("verus:")}
            for value in basis
            if value.startswith("verus:")
        )
        typed_basis.extend(
            {"kind": "bounded-conformance", "id": vectors}
            for _ in [0]
            if vectors != "-"
        )
        typed_basis.extend(
            {"kind": "supporting-smt", "id": obligation_id}
            for obligation_id in claim_smt_ids
        )
        typed_basis.extend(
            {"kind": "supporting-halmos", "id": obligation_id}
            for obligation_id in claim_halmos_ids
        )
        evidence = {
            "proof": (
                "proved" if manifest.contracts[claim_id].is_proved else "unproved"
            ),
            "implementation": (
                "implementation-proved"
                if implementation_strength == "implementation-proved"
                else implementation_strength
            ),
            "tests": "tested" if tests else "unproved",
            "assumptions": (
                "assumed" if items(assumption_ids) else "not-applicable"
            ),
            "contract": (
                "proved" if manifest.contracts[claim_id].is_proved else "unproved"
            ),
            "proof_class": manifest.contracts[claim_id].proof_class,
            "abstract": abstract_evidence_status(abstract_theorems),
            "production_kernel": "ownership-registered",
            "smt_scalar": [
                {
                    "id": obligation_id,
                    "status": "supporting-proved",
                }
                for obligation_id in claim_smt_ids
            ],
            "halmos_commit": [
                {
                    "id": obligation_id,
                    "status": "supporting-symbolically-proved",
                    "boundary": "post-auth-commit",
                }
                for obligation_id in claim_halmos_ids
            ],
            "implementation_basis": basis,
            "typed_implementation_basis": typed_basis,
            "adapter": "transaction-tested" if tests else "missing",
            "vector_consumer": vector_consumer,
            "external": "assumed" if items(assumption_ids) else "not-applicable",
        }
        reasons: list[str] = []
        if not manifest.contracts[claim_id].is_proved:
            reasons.append("missing_claim_contract")
        if uncovered_verus:
            reasons.append(
                "uncovered_verus:" + ",".join(sorted(uncovered_verus))
            )
        if not basis:
            reasons.append("implementation_proof_gap:missing_formal_basis")
        if unreferenced_kernels:
            reasons.append(
                "production_kernel_not_linked:" + ",".join(sorted(unreferenced_kernels))
            )
        if items(assumption_ids):
            reasons.append("external_assumptions:" + ",".join(items(assumption_ids)))
        release_gaps: list[str] = []
        if not registration.is_proved:
            release_gaps.append("missing_claim_contract")
        if not production:
            release_gaps.append("missing_production_symbols")
        if not tests:
            release_gaps.append("missing_transaction_tests")
        if unreferenced_kernels:
            release_gaps.append("production_kernel_not_linked")
        if not required_strength_met(
            implementation_strength, registration.required_strength
        ):
            release_gaps.append(
                f"required_strength:{registration.required_strength}"
            )
        status = (
            "model-support"
            if registration.assurance_target == "model-support"
            else "release-blocked" if release_gaps else "release-ready"
        )
        results.append(
            {
                "id": claim_id,
                "kind": kind,
                "status": status,
                "assurance_target": registration.assurance_target,
                "required_strength": registration.required_strength,
                "evidence_strength": implementation_strength,
                "evidence": evidence,
                "unproved_reasons": reasons,
                "release_blockers": release_gaps,
            }
        )

    missing_verus = set(verus_rows) - used_verus
    if missing_verus:
        raise ValueError(
            f"unregistered Verus obligations in unified claims: {sorted(missing_verus)}"
        )
    require_exact_claim_coverage(
        "Verus",
        {
            obligation.proof: set(obligation.claim_ids)
            for obligation in verus_manifest.values()
        },
        referenced_verus_claims,
    )
    require_exact_claim_coverage(
        "SMT",
        {
            obligation_id: set(obligation.claim_ids)
            for obligation_id, obligation in smt_obligations_by_id.items()
        },
        referenced_smt_claims,
    )
    require_exact_claim_coverage(
        "Halmos",
        {
            obligation_id: set(obligation.claim_ids)
            for obligation_id, obligation in halmos_obligations_by_id.items()
        },
        referenced_halmos_claims,
    )
    for assumption, declared in assumption_dependencies.items():
        actual = actual_assumption_dependencies[assumption]
        if declared != actual:
            raise ValueError(
                f"external assumption dependency mismatch for {assumption}: "
                f"declared={sorted(declared)} actual={sorted(actual)}"
            )
    return {
        "schema": CLAIM_REPORT_SCHEMA,
        "source_fingerprint": source_fingerprint(),
        "claims": results,
        "conditional_liveness": [
            {
                "id": property.property_id,
                "status": "conditional-liveness",
                "theorem": property.theorem,
                "assumptions": list(property.assumption_ids),
                "implementation": "unproved",
            }
            for property in conditional_liveness.values()
        ],
    }


def write_claim_report(report: dict[str, object], path: Path = REPORT) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")


def require_release_ready_catalog(results: list[dict[str, object]]) -> None:
    ready = {
        str(claim["id"])
        for claim in results
        if claim["status"] == "release-ready"
    }
    if ready != REQUIRED_CLAIM_IDS:
        raise ValueError(
            "release safety catalog is not fully ready: "
            f"missing={sorted(REQUIRED_CLAIM_IDS - ready)} "
            f"extra={sorted(ready - REQUIRED_CLAIM_IDS)}"
        )


def main() -> int:
    check_operational_config_guard_dominance()
    report = build_claim_report()
    write_claim_report(report)
    results = report["claims"]
    assert isinstance(results, list)
    require_release_ready_catalog(results)
    print(f"unified claim manifest passed ({len(results)} claims)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
