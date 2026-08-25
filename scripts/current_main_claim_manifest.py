#!/usr/bin/env python3
"""Parse the fail-closed v3 claim and proof-contract manifest."""

from __future__ import annotations

import re
from dataclasses import dataclass


SCHEMA_VERSION = "3"
CLAIM_FIELD_COUNT = 11
LEAN_NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*")
PROOF_CLASSES = {"local-safety", "history-safety", "liveness", "implementation-only"}


@dataclass(frozen=True)
class ContractRegistration:
    claim_id: str
    proof_class: str
    contract: str
    witness: str

    @property
    def is_proved(self) -> bool:
        return self.contract != "-" and self.witness != "-"


@dataclass(frozen=True)
class ClaimManifest:
    rows: tuple[tuple[str, ...], ...]
    contracts: dict[str, ContractRegistration]


def parse_claim_manifest(text: str) -> ClaimManifest:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != ["schema", SCHEMA_VERSION, "-", "-", "-"]:
        raise ValueError(f"claim manifest must start with schema {SCHEMA_VERSION}")

    rows: list[tuple[str, ...]] = []
    contracts: dict[str, ContractRegistration] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = tuple(line.split("\t"))
        if fields[0] == "contract":
            if len(fields) != 5 or not all(fields):
                raise ValueError(f"invalid claim contract row {number}")
            _, claim_id, proof_class, contract, witness = fields
            if claim_id in contracts:
                raise ValueError(f"duplicate claim contract: {claim_id}")
            if proof_class not in PROOF_CLASSES:
                raise ValueError(f"invalid proof class for {claim_id}: {proof_class}")
            if (contract == "-") != (witness == "-"):
                raise ValueError(f"claim contract and witness must be paired: {claim_id}")
            if contract != "-" and (
                LEAN_NAME.fullmatch(contract) is None or LEAN_NAME.fullmatch(witness) is None
            ):
                raise ValueError(f"invalid Lean claim contract registration: {claim_id}")
            if contract in {"True", "Bool.true"}:
                raise ValueError(f"vacuous Lean claim contract: {claim_id}")
            contracts[claim_id] = ContractRegistration(
                claim_id, proof_class, contract, witness
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


def lean_contract_check_source(manifest: ClaimManifest) -> str:
    lines = ["import BridgeSpec.ClaimContracts", ""]
    for claim_id in sorted(manifest.contracts):
        registration = manifest.contracts[claim_id]
        if registration.is_proved:
            lines.append(
                f"example : {registration.contract} := {registration.witness}"
            )
    return "\n".join(lines) + "\n"
