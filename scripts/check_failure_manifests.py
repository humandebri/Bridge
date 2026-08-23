#!/usr/bin/env python3
"""Validate deliberate proof failures are registered one-to-one with claims."""

from __future__ import annotations

import re
from pathlib import Path

from smt_obligations import parse_smt_obligations
from halmos_obligations import parse_halmos_obligations
from check_claim_manifest import checked_link

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
    smt_obligations = parse_smt_obligations(
        (ROOT / "verification" / "smt" / "obligations.tsv").read_text(encoding="utf-8")
    )
    registered_failure_ids = {
        failure_id
        for obligation in smt_obligations.values()
        for failure_id in obligation.failure_ids
    }
    actual_failure_ids = {row[0] for row in smt_rows}
    if registered_failure_ids != actual_failure_ids:
        raise ValueError(
            "SMT obligations do not exactly cover negative IDs: "
            f"missing={sorted(actual_failure_ids - registered_failure_ids)} "
            f"extra={sorted(registered_failure_ids - actual_failure_ids)}"
        )

    halmos_dir = ROOT / "contracts" / "test" / "halmos" / "fail"
    halmos_rows = rows(ROOT / "verification" / "halmos" / "failure-manifest.tsv", 2)
    halmos_fixture_paths: list[str] = []
    for failure_id, link in halmos_rows:
        path, _ = checked_link(link)
        if halmos_dir.resolve() not in path.parents:
            raise ValueError(f"Halmos failure fixture is outside the fail directory: {link}")
        halmos_fixture_paths.append(path.relative_to(halmos_dir).as_posix())
    actual_halmos = sorted(
        path.relative_to(halmos_dir).as_posix()
        for path in halmos_dir.rglob("*.sol")
        if path.is_file()
    )
    if (
        len({row[0] for row in halmos_rows}) != len(halmos_rows)
        or len(set(halmos_fixture_paths)) != len(halmos_fixture_paths)
        or sorted(halmos_fixture_paths) != actual_halmos
    ):
        raise ValueError("Halmos failure manifest does not exactly cover deliberate fixtures")
    halmos_obligations = parse_halmos_obligations(
        (ROOT / "verification" / "halmos" / "obligations.tsv").read_text(encoding="utf-8")
    )
    registered_halmos_failure_ids = {
        failure_id
        for obligation in halmos_obligations.values()
        for failure_id in obligation.failure_ids
    }
    actual_halmos_failure_ids = {row[0] for row in halmos_rows}
    if registered_halmos_failure_ids != actual_halmos_failure_ids:
        raise ValueError(
            "Halmos obligations do not exactly cover negative IDs: "
            f"missing={sorted(actual_halmos_failure_ids - registered_halmos_failure_ids)} "
            f"extra={sorted(registered_halmos_failure_ids - actual_halmos_failure_ids)}"
        )

    lean_dir = ROOT / "verification" / "lean" / "fail"
    lean_source = "\n".join(
        (ROOT / "verification" / "lean" / "BridgeSpec" / name).read_text(encoding="utf-8")
        for name in (
            "DepositAuthorization.lean",
            "ClaimContracts.lean",
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
