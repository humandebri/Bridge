#!/usr/bin/env python3
"""Validate deliberate proof failures are registered one-to-one with claims."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def rows(path: Path, width: int) -> list[list[str]]:
    parsed = [line.split("\t") for line in path.read_text(encoding="utf-8").splitlines() if line]
    if any(len(row) != width or not all(row) for row in parsed):
        raise ValueError(f"invalid failure manifest row: {path}")
    return parsed


def main() -> int:
    smt_dir = ROOT / "verification" / "smt" / "fail"
    smt_rows = rows(ROOT / "verification" / "smt" / "failure-manifest.tsv", 2)
    smt_fixtures = [row[1] for row in smt_rows]
    actual_smt = sorted(path.name for path in smt_dir.glob("*.sol"))
    if len(set(smt_fixtures)) != len(smt_fixtures) or sorted(smt_fixtures) != actual_smt:
        raise ValueError("SMT failure manifest does not exactly cover deliberate fixtures")

    lean_dir = ROOT / "verification" / "lean" / "fail"
    lean_source = "\n".join(
        (ROOT / "verification" / "lean" / "BridgeSpec" / name).read_text(encoding="utf-8")
        for name in (
            "DepositAuthorization.lean",
            "LedgerBlockProvenance.lean",
            "Protocol.lean",
        )
    )
    lean_rows = rows(ROOT / "verification" / "lean" / "deposit-failure-manifest.tsv", 3)
    seen_pairs: set[tuple[str, str]] = set()
    for theorem, fixture, missing_premise in lean_rows:
        pair = theorem, missing_premise
        if pair in seen_pairs or not re.fullmatch(r"[a-z0-9_]+", missing_premise):
            raise ValueError(f"duplicate or invalid Lean failure mapping: {pair}")
        seen_pairs.add(pair)
        if re.search(rf"^theorem {re.escape(theorem)}\b", lean_source, re.MULTILINE) is None:
            raise ValueError(f"unknown Lean theorem in failure manifest: {theorem}")
        if not (lean_dir / fixture).is_file():
            raise ValueError(f"missing Lean failure fixture: {fixture}")
    print("failure fixture manifests passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
