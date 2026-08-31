#!/usr/bin/env python3
"""Parse the fail-closed claim and proof-contract manifest."""

from __future__ import annotations

import re
from dataclasses import dataclass


SCHEMA_VERSION = "6"
CLAIM_FIELD_COUNT = 13
LEAN_NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*")
PROOF_CLASSES = {"local-safety", "history-safety", "implementation-only"}
ASSURANCE_TARGETS = {"release-safety", "model-support"}
REQUIRED_STRENGTHS = {"production-linked", "implementation-proved"}
REQUIRED_CLAIM_IDS = frozenset(
    """activation_preflight authorization_binding canonical_probe committed_quote
    deposit_admission deposit_backing deposit_identity_preflight epoch_invalidation
    exact_mint_finalization expiry_refund
    fee_accounting_once fee_payout fee_recipient_rotation funding_attempt_lifecycle
    funding_reconciliation_freshness
    governance_confirmation_authorization governance_nonce_chain_binding
    governance_transaction_affordability hold_resolution lease_lane_isolation
    lease_outcome ledger_block_provenance nonterminal_deposit_index_consistency
    notification_quota_isolation payment_identity pending_queue
    operational_config_seal
    refund_evidence_enforcement refund_request_authorization reservation_commit
    reservation_lifecycle runtime_attestation_reuse service_fee_maximum
    settlement_backing signing_cycle_reserve withdrawal_admission_boundary
    withdrawal_finality_quorum withdrawal_finalization""".split()
)
REQUIRED_IMPLEMENTATION_PROVED_CLAIM_IDS = frozenset(
    """activation_preflight canonical_probe committed_quote deposit_identity_preflight
    fee_recipient_rotation funding_attempt_lifecycle funding_reconciliation_freshness
    governance_confirmation_authorization governance_transaction_affordability
    lease_lane_isolation ledger_block_provenance nonterminal_deposit_index_consistency
    notification_quota_isolation operational_config_seal refund_request_authorization reservation_commit
    runtime_attestation_reuse service_fee_maximum signing_cycle_reserve
    withdrawal_admission_boundary""".split()
)
REQUIRED_CLAIM_POLICY = {
    claim_id: (
        "release-safety",
        "implementation-proved"
        if claim_id in REQUIRED_IMPLEMENTATION_PROVED_CLAIM_IDS
        else "production-linked",
    )
    for claim_id in REQUIRED_CLAIM_IDS
}

REQUIRED_CONDITIONAL_LIVENESS_POLICY = {
    "withdrawal_eventually_paid": (
        "BridgeSpec.Liveness.committed_withdrawal_eventually_paid",
        "BridgeSpec.Liveness.WithdrawalEventuallyPaid",
        frozenset(
            {
                "eventual_external_resolution",
                "eventual_keeper_action",
                "eventual_storage_commit",
                "ledger_fee_immutability",
                "no_permanent_pause",
                "runtime_toolchain",
                "scheduler_weak_fairness",
                "time_progress_and_cycles",
                "terminal_transition_admissibility",
            }
        ),
    ),
    "funded_deposit_eventually_minted": (
        "BridgeSpec.Liveness.funded_deposit_eventually_minted",
        "BridgeSpec.Liveness.FundedDepositEventuallyMinted",
        frozenset(
            {
                "eventual_external_resolution",
                "eventual_storage_commit",
                "eventual_user_action",
                "no_permanent_pause",
                "runtime_toolchain",
                "scheduler_weak_fairness",
                "time_progress_and_cycles",
                "terminal_transition_admissibility",
            }
        ),
    ),
    "expired_deposit_eventually_refunded": (
        "BridgeSpec.Liveness.expired_deposit_eventually_refunded",
        "BridgeSpec.Liveness.ExpiredDepositEventuallyRefunded",
        frozenset(
            {
                "eventual_external_resolution",
                "eventual_keeper_action",
                "eventual_storage_commit",
                "ledger_fee_immutability",
                "no_permanent_pause",
                "runtime_toolchain",
                "scheduler_weak_fairness",
                "time_progress_and_cycles",
                "terminal_transition_admissibility",
            }
        ),
    ),
    "funded_deposit_eventually_minted_or_refunded": (
        "BridgeSpec.Liveness.funded_deposit_eventually_minted_or_refunded",
        "BridgeSpec.Liveness.FundedDepositEventuallyMintedOrRefunded",
        frozenset(
            {
                "eventual_external_resolution",
                "eventual_keeper_action",
                "eventual_storage_commit",
                "eventual_user_action",
                "ledger_fee_immutability",
                "no_permanent_pause",
                "runtime_toolchain",
                "scheduler_weak_fairness",
                "time_progress_and_cycles",
                "terminal_transition_admissibility",
            }
        ),
    ),
    "funding_failure_eventually_cancelled": (
        "BridgeSpec.Liveness.funding_failure_eventually_cancelled",
        "BridgeSpec.Liveness.FundingFailureEventuallyCancelled",
        frozenset(
            {
                "eventual_external_resolution",
                "eventual_storage_commit",
                "no_permanent_pause",
                "runtime_toolchain",
                "scheduler_weak_fairness",
                "time_progress_and_cycles",
                "terminal_transition_admissibility",
            }
        ),
    ),
}
REQUIRED_CONDITIONAL_LIVENESS_IDS = frozenset(
    REQUIRED_CONDITIONAL_LIVENESS_POLICY
)


@dataclass(frozen=True)
class ContractRegistration:
    claim_id: str
    proof_class: str
    assurance_target: str
    required_strength: str
    contract: str
    witness: str

    @property
    def is_proved(self) -> bool:
        return self.contract != "-" and self.witness != "-"


@dataclass(frozen=True)
class ClaimManifest:
    rows: tuple[tuple[str, ...], ...]
    contracts: dict[str, ContractRegistration]


@dataclass(frozen=True)
class ConditionalLivenessProperty:
    property_id: str
    theorem: str
    proposition: str
    assumption_ids: tuple[str, ...]


def parse_claim_manifest(text: str) -> ClaimManifest:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != [
        "schema", SCHEMA_VERSION, "-", "-", "-", "-", "-"
    ]:
        raise ValueError(f"claim manifest must start with schema {SCHEMA_VERSION}")

    rows: list[tuple[str, ...]] = []
    contracts: dict[str, ContractRegistration] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = tuple(line.split("\t"))
        if fields[0] == "contract":
            if len(fields) != 7 or not all(fields):
                raise ValueError(f"invalid claim contract row {number}")
            (
                _, claim_id, proof_class, assurance_target, required_strength,
                contract, witness,
            ) = fields
            if claim_id in contracts:
                raise ValueError(f"duplicate claim contract: {claim_id}")
            if proof_class not in PROOF_CLASSES:
                raise ValueError(f"invalid proof class for {claim_id}: {proof_class}")
            if assurance_target not in ASSURANCE_TARGETS:
                raise ValueError(
                    f"invalid assurance target for {claim_id}: {assurance_target}"
                )
            if required_strength not in REQUIRED_STRENGTHS:
                raise ValueError(
                    f"invalid required strength for {claim_id}: {required_strength}"
                )
            if assurance_target == "release-safety" and contract == "-":
                raise ValueError(
                    f"release safety claim requires a Lean contract: {claim_id}"
                )
            if (contract == "-") != (witness == "-"):
                raise ValueError(f"claim contract and witness must be paired: {claim_id}")
            if contract != "-" and (
                LEAN_NAME.fullmatch(contract) is None or LEAN_NAME.fullmatch(witness) is None
            ):
                raise ValueError(f"invalid Lean claim contract registration: {claim_id}")
            if contract in {"True", "Bool.true"}:
                raise ValueError(f"vacuous Lean claim contract: {claim_id}")
            contracts[claim_id] = ContractRegistration(
                claim_id, proof_class, assurance_target, required_strength,
                contract, witness
            )
        else:
            if len(fields) != CLAIM_FIELD_COUNT or not all(fields):
                raise ValueError(f"invalid claim row {number}")
            rows.append(fields)

    claim_ids = [row[1] for row in rows]
    if len(claim_ids) != len(set(claim_ids)):
        raise ValueError("duplicate unified claim id")
    if set(claim_ids) != set(contracts):
        raise ValueError(
            "claim contract coverage differs from claims: "
            f"missing={sorted(set(claim_ids) - set(contracts))} "
            f"extra={sorted(set(contracts) - set(claim_ids))}"
        )
    return ClaimManifest(tuple(rows), contracts)


def parse_conditional_liveness_manifest(
    text: str,
) -> dict[str, ConditionalLivenessProperty]:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != ["schema", "1", "-", "-"]:
        raise ValueError("conditional liveness manifest must start with schema 1")
    properties: dict[str, ConditionalLivenessProperty] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = tuple(line.split("\t"))
        if len(fields) != 4 or fields[0] != "property" or not all(fields):
            raise ValueError(f"invalid conditional liveness row {number}")
        _, property_id, theorem, raw_assumptions = fields
        if property_id in properties or LEAN_NAME.fullmatch(theorem) is None:
            raise ValueError(f"invalid conditional liveness property: {property_id}")
        assumption_ids = tuple(raw_assumptions.split(";"))
        if len(assumption_ids) != len(set(assumption_ids)):
            raise ValueError(f"duplicate conditional liveness assumption: {property_id}")
        policy = REQUIRED_CONDITIONAL_LIVENESS_POLICY.get(property_id)
        if policy is None:
            raise ValueError(f"invalid conditional liveness property: {property_id}")
        expected_theorem, proposition, expected_assumptions = policy
        if theorem != expected_theorem:
            raise ValueError(
                f"conditional liveness theorem differs for {property_id}: "
                f"expected={expected_theorem} actual={theorem}"
            )
        if set(assumption_ids) != expected_assumptions:
            raise ValueError(
                f"conditional liveness assumptions differ for {property_id}: "
                f"missing={sorted(expected_assumptions - set(assumption_ids))} "
                f"extra={sorted(set(assumption_ids) - expected_assumptions)}"
            )
        properties[property_id] = ConditionalLivenessProperty(
            property_id, theorem, proposition, assumption_ids
        )
    if set(properties) != REQUIRED_CONDITIONAL_LIVENESS_IDS:
        raise ValueError(
            "conditional liveness catalog differs: "
            f"missing={sorted(REQUIRED_CONDITIONAL_LIVENESS_IDS - set(properties))} "
            f"extra={sorted(set(properties) - REQUIRED_CONDITIONAL_LIVENESS_IDS)}"
        )
    return properties


def conditional_liveness_check_source(
    properties: dict[str, ConditionalLivenessProperty],
) -> str:
    lines = ["import BridgeSpec.Liveness", ""]
    for property_id in sorted(properties):
        property = properties[property_id]
        lines.extend(
            [
                f"example : {property.proposition} := by",
                f"  exact {property.theorem}",
                f"#print axioms {property.theorem}",
            ]
        )
    return "\n".join(lines) + "\n"


def lean_contract_check_source(manifest: ClaimManifest) -> str:
    lines = ["import BridgeSpec.ClaimContracts", ""]
    for claim_id in sorted(manifest.contracts):
        registration = manifest.contracts[claim_id]
        if registration.is_proved:
            lines.extend(
                [
                    f"example : {registration.contract} := by",
                    "  fail_if_success exact True.intro",
                    f"  exact {registration.witness}",
                    f"#print axioms {registration.witness}",
                ]
            )
    return "\n".join(lines) + "\n"
