#!/usr/bin/env python3
"""Parse the typed Solidity SMT obligation manifest."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path
import re

from trusted_proof_profiles import require_profile


SCHEMA_VERSION = "2"
IDENTIFIER = re.compile(r"[a-z][a-z0-9_]*")
STRENGTHS = {"supporting"}
ROOT = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class SmtObligation:
    obligation_id: str
    strength: str
    pass_links: tuple[str, ...]
    production_links: tuple[str, ...]
    failure_ids: tuple[str, ...]
    claim_ids: tuple[str, ...]


def _items(value: str) -> tuple[str, ...]:
    return () if value == "-" else tuple(value.split(";"))


def parse_smt_obligations(text: str) -> dict[str, SmtObligation]:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != ["schema", SCHEMA_VERSION, "-", "-", "-", "-", "-"]:
        raise ValueError(f"SMT obligation manifest must start with schema {SCHEMA_VERSION}")

    obligations: dict[str, SmtObligation] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 7 or fields[0] != "obligation" or not all(fields):
            raise ValueError(f"invalid SMT obligation row {number}")
        _, obligation_id, strength, pass_links, production_links, failure_ids, claim_ids = fields
        if IDENTIFIER.fullmatch(obligation_id) is None or obligation_id in obligations:
            raise ValueError(f"invalid or duplicate SMT obligation: {obligation_id}")
        if strength not in STRENGTHS:
            raise ValueError(f"invalid SMT obligation strength: {obligation_id}/{strength}")
        parsed = SmtObligation(
            obligation_id,
            strength,
            _items(pass_links),
            _items(production_links),
            _items(failure_ids),
            _items(claim_ids),
        )
        if (
            not parsed.pass_links
            or not parsed.production_links
            or not parsed.failure_ids
            or not parsed.claim_ids
        ):
            raise ValueError(f"incomplete SMT obligation: {obligation_id}")
        for label, values in (
            ("pass links", parsed.pass_links),
            ("production links", parsed.production_links),
            ("failure IDs", parsed.failure_ids),
            ("claim IDs", parsed.claim_ids),
        ):
            if len(values) != len(set(values)):
                raise ValueError(f"duplicate SMT {label}: {obligation_id}")
        if any(IDENTIFIER.fullmatch(value) is None for value in parsed.failure_ids):
            raise ValueError(f"invalid SMT failure ID: {obligation_id}")
        if any(IDENTIFIER.fullmatch(value) is None for value in parsed.claim_ids):
            raise ValueError(f"invalid SMT claim ID: {obligation_id}")
        obligations[obligation_id] = parsed
    return obligations


def validate_trusted_smt_sources(
    root: Path = ROOT, expected: dict[str, str] | None = None
) -> None:
    if expected is None:
        require_profile("security-hardening-v1", root)
        return
    for relative, digest in expected.items():
        path = root / relative
        if not path.is_file():
            raise ValueError(f"missing trusted SMT source: {relative}")
        actual = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual != digest:
            raise ValueError(f"trusted SMT source digest differs: {relative}")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check-sources", action="store_true", required=True)
    parser.parse_args()
    validate_trusted_smt_sources()
    print("trusted SMT sources passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
