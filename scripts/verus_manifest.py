#!/usr/bin/env python3
"""Parse the typed Verus implementation-binding manifest."""

from __future__ import annotations

from dataclasses import dataclass
import re


SCHEMA_VERSION = "4"
IDENTIFIER = re.compile(r"[a-z][a-z0-9_]*")
KINDS = {"executable", "shared-expression", "derived", "model"}


@dataclass(frozen=True)
class VerusObligation:
    obligation_id: str
    kind: str
    kernel: str
    proof: str
    fixture: str
    binding: tuple[str, ...]
    derived_bindings: tuple[tuple[int, int, str], ...]
    call_sites: tuple[str, ...]
    claim_ids: tuple[str, ...]

    @property
    def production_bound(self) -> bool:
        return self.kind in {"executable", "shared-expression"} and bool(self.call_sites)


def _items(value: str) -> tuple[str, ...]:
    return () if value == "-" else tuple(value.split(";"))


def parse_verus_manifest(text: str) -> dict[str, VerusObligation]:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != [
        "schema", SCHEMA_VERSION, "-", "-", "-", "-", "-", "-", "-"
    ]:
        raise ValueError(f"Verus manifest must start with schema {SCHEMA_VERSION}")
    obligations: dict[str, VerusObligation] = {}
    kernels: set[str] = set()
    proofs: set[str] = set()
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 9 or not all(fields):
            raise ValueError(f"invalid Verus manifest row {number}")
        (
            obligation_id,
            kind,
            kernel,
            proof,
            fixture,
            binding,
            derived_bindings,
            call_sites,
            claim_ids,
        ) = fields
        if IDENTIFIER.fullmatch(obligation_id) is None or obligation_id in obligations:
            raise ValueError(f"invalid or duplicate Verus obligation: {obligation_id}")
        if kind not in KINDS or kernel in kernels:
            raise ValueError(f"invalid Verus kind or duplicate kernel: {obligation_id}/{kind}/{kernel}")
        if proof in proofs:
            raise ValueError(f"duplicate Verus proof: {proof}")
        parsed_binding = _items(binding)
        parsed_derived: list[tuple[int, int, str]] = []
        for value in _items(derived_bindings):
            fields = value.split(":", 2)
            if len(fields) != 3 or not fields[0].isdigit() or not fields[1].isdigit() or not fields[2]:
                raise ValueError(f"invalid Verus derived binding: {obligation_id}/{value}")
            parsed_derived.append((int(fields[0]), int(fields[1]), fields[2]))
        if len({item[0] for item in parsed_derived}) != len(parsed_derived):
            raise ValueError(f"duplicate Verus derived binding position: {obligation_id}")
        parsed_calls = _items(call_sites)
        parsed_claims = _items(claim_ids)
        if not parsed_claims:
            raise ValueError(f"Verus obligation must declare claims: {obligation_id}")
        if len(parsed_claims) != len(set(parsed_claims)):
            raise ValueError(f"duplicate Verus claim IDs: {obligation_id}")
        if any(IDENTIFIER.fullmatch(claim_id) is None for claim_id in parsed_claims):
            raise ValueError(f"invalid Verus claim ID: {obligation_id}")
        if kind == "executable" and parsed_binding != ("direct",):
            raise ValueError(f"executable Verus obligation must use direct binding: {obligation_id}")
        if kind == "shared-expression" and len(parsed_binding) != 1:
            raise ValueError(f"shared-expression must name one macro: {obligation_id}")
        if kind != "shared-expression" and parsed_derived:
            raise ValueError(f"only shared-expression may register derived bindings: {obligation_id}")
        if kind == "derived" and not parsed_binding:
            raise ValueError(f"derived Verus obligation must name dependencies: {obligation_id}")
        if kind == "model" and (parsed_binding or parsed_calls):
            raise ValueError(f"model Verus obligation cannot bind production: {obligation_id}")
        obligation = VerusObligation(
            obligation_id,
            kind,
            kernel,
            proof,
            fixture,
            parsed_binding,
            tuple(parsed_derived),
            parsed_calls,
            parsed_claims,
        )
        obligations[obligation_id] = obligation
        kernels.add(kernel)
        proofs.add(proof)
    return obligations
