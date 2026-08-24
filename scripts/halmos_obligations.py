#!/usr/bin/env python3
"""Parse the typed Halmos post-authentication obligation manifest."""

from __future__ import annotations

from dataclasses import dataclass
import re


SCHEMA_VERSION = "1"
IDENTIFIER = re.compile(r"[a-z][a-z0-9_]*")
STRENGTHS = {"supporting"}


@dataclass(frozen=True)
class HalmosObligation:
    obligation_id: str
    strength: str
    test_links: tuple[str, ...]
    production_links: tuple[str, ...]
    failure_ids: tuple[str, ...]
    claim_ids: tuple[str, ...]

    @property
    def claim_complete(self) -> bool:
        return self.strength == "claim-complete"


def _items(value: str) -> tuple[str, ...]:
    return () if value == "-" else tuple(value.split(";"))


def parse_halmos_obligations(text: str) -> dict[str, HalmosObligation]:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != [
        "schema",
        SCHEMA_VERSION,
        "-",
        "-",
        "-",
        "-",
        "-",
    ]:
        raise ValueError(f"Halmos obligation manifest must start with schema {SCHEMA_VERSION}")

    obligations: dict[str, HalmosObligation] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 7 or fields[0] != "obligation" or not all(fields):
            raise ValueError(f"invalid Halmos obligation row {number}")
        _, obligation_id, strength, test_links, production_links, failure_ids, claim_ids = fields
        if IDENTIFIER.fullmatch(obligation_id) is None or obligation_id in obligations:
            raise ValueError(f"invalid or duplicate Halmos obligation: {obligation_id}")
        if strength not in STRENGTHS:
            raise ValueError(f"invalid Halmos obligation strength: {obligation_id}/{strength}")
        parsed = HalmosObligation(
            obligation_id,
            strength,
            _items(test_links),
            _items(production_links),
            _items(failure_ids),
            _items(claim_ids),
        )
        for label, values in (
            ("test links", parsed.test_links),
            ("production links", parsed.production_links),
            ("failure IDs", parsed.failure_ids),
            ("claim IDs", parsed.claim_ids),
        ):
            if not values:
                raise ValueError(f"incomplete Halmos obligation: {obligation_id}/{label}")
            if len(values) != len(set(values)):
                raise ValueError(f"duplicate Halmos {label}: {obligation_id}")
        if any(IDENTIFIER.fullmatch(value) is None for value in parsed.failure_ids):
            raise ValueError(f"invalid Halmos failure ID: {obligation_id}")
        if any(IDENTIFIER.fullmatch(value) is None for value in parsed.claim_ids):
            raise ValueError(f"invalid Halmos claim ID: {obligation_id}")
        obligations[obligation_id] = parsed
    return obligations
