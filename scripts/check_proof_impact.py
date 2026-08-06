#!/usr/bin/env python3
"""Validate logic-to-proof ownership and proof-receipt source freshness."""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from proof_fingerprint import (
    FINGERPRINT_CONFIG_FILES,
    FINGERPRINT_SOURCE_ROOTS,
    fingerprint_inputs,
    source_fingerprint,
)


ROOT = Path(__file__).resolve().parents[1]
REQUIRED_STAGES = (
    "claim-manifest",
    "lean",
    "lean-negative",
    "policy-vector-consumers",
    "refinement-gate",
    "claim-transaction-tests",
    "known-answer-consumers",
    "smt-and-negative",
    "verus-and-negative",
)
RECEIPT_SCHEMA = 4


@dataclass(frozen=True)
class WatchedRoot:
    identifier: str
    path: str
    suffix: str


@dataclass(frozen=True)
class ImpactArea:
    identifier: str
    sources: tuple[str, ...]
    claims: tuple[str, ...]
    stages: tuple[str, ...]


@dataclass(frozen=True)
class ImpactManifest:
    roots: tuple[WatchedRoot, ...]
    areas: tuple[ImpactArea, ...]


def _parts(value: str) -> tuple[str, ...]:
    return tuple(value.split(";"))


def _claim_catalog(repo_root: Path) -> dict[str, str]:
    catalog: dict[str, str] = {}
    for line in (repo_root / "verification" / "claims.tsv").read_text(
        encoding="utf-8"
    ).splitlines():
        row = line.split("\t")
        if len(row) != 11:
            raise ValueError("claim manifest must be valid before impact resolution")
        kind, claim_id = row[:2]
        if claim_id in catalog:
            raise ValueError(f"duplicate claim id: {claim_id}")
        catalog[claim_id] = kind
    return catalog


def _claim_production_sources(repo_root: Path) -> set[str]:
    sources: set[str] = set()
    for line in (repo_root / "verification" / "claims.tsv").read_text(
        encoding="utf-8"
    ).splitlines():
        row = line.split("\t")
        if len(row) != 11:
            raise ValueError("claim manifest must be valid before impact resolution")
        for link in row[7].split(";"):
            if link == "-" or link.count("#") != 1:
                raise ValueError(f"invalid production source link: {link}")
            sources.add(link.split("#", 1)[0])
    return sources


def _resolve_claims(
    selectors: tuple[str, ...], catalog: dict[str, str]
) -> tuple[str, ...]:
    resolved: set[str] = set()
    for selector in selectors:
        if selector.endswith(":*"):
            kind = selector[:-2]
            matches = {
                claim for claim, claim_kind in catalog.items() if claim_kind == kind
            }
            if not matches:
                raise ValueError(f"claim selector matches nothing: {selector}")
            resolved.update(matches)
        elif selector in catalog:
            resolved.add(selector)
        else:
            raise ValueError(f"unknown impact claim selector: {selector}")
    return tuple(sorted(resolved))


def load_manifest(repo_root: Path = ROOT) -> ImpactManifest:
    manifest_path = repo_root / "verification" / "proof-impact.tsv"
    roots: list[WatchedRoot] = []
    raw_areas: list[ImpactArea] = []
    for number, line in enumerate(
        manifest_path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        row = line.split("\t")
        if len(row) != 5 or not all(row):
            raise ValueError(f"invalid proof impact row {number}")
        kind, identifier, value1, value2, value3 = row
        if kind == "root":
            if value3 != "-" or not value2.startswith("."):
                raise ValueError(f"invalid watched root row {number}")
            roots.append(WatchedRoot(identifier, value1.rstrip("/"), value2))
        elif kind == "area":
            raw_areas.append(
                ImpactArea(identifier, _parts(value1), _parts(value2), _parts(value3))
            )
        else:
            raise ValueError(f"unknown proof impact row kind at {number}: {kind}")

    root_ids = [item.identifier for item in roots]
    area_ids = [item.identifier for item in raw_areas]
    if len(root_ids) != len(set(root_ids)):
        raise ValueError("duplicate watched root identifier")
    if len(area_ids) != len(set(area_ids)):
        raise ValueError("duplicate impact area identifier")
    if not roots or not raw_areas:
        raise ValueError("proof impact manifest requires roots and areas")

    catalog = _claim_catalog(repo_root)
    areas = tuple(
        ImpactArea(
            area.identifier,
            area.sources,
            _resolve_claims(area.claims, catalog),
            area.stages,
        )
        for area in raw_areas
    )
    expected_stages = set(REQUIRED_STAGES)
    for area in areas:
        if set(area.stages) != expected_stages or len(area.stages) != len(
            expected_stages
        ):
            raise ValueError(
                f"impact area must require the complete proof suite: {area.identifier}"
            )

    registered_sources = [source for area in areas for source in area.sources]
    if len(registered_sources) != len(set(registered_sources)):
        raise ValueError("a safety source is registered by multiple impact areas")
    for source in registered_sources:
        if not (repo_root / source).is_file():
            raise ValueError(f"missing registered safety source: {source}")
    missing_production_sources = _claim_production_sources(repo_root) - set(
        registered_sources
    )
    if missing_production_sources:
        raise ValueError(
            "claim production sources missing proof impact ownership: "
            f"{sorted(missing_production_sources)}"
        )

    expected_sources: set[str] = set()
    for watched in roots:
        watched_path = repo_root / watched.path
        if not watched_path.is_dir():
            raise ValueError(f"missing watched safety root: {watched.path}")
        expected_sources.update(
            path.relative_to(repo_root).as_posix()
            for path in watched_path.rglob(f"*{watched.suffix}")
            if path.is_file()
        )
    missing = expected_sources - set(registered_sources)
    if missing:
        raise ValueError(f"unregistered safety sources: {sorted(missing)}")
    return ImpactManifest(tuple(roots), areas)


def classify_paths(
    paths: list[str], manifest: ImpactManifest, *, reject_unregistered: bool = True
) -> dict[str, object]:
    source_to_area = {
        source: area for area in manifest.areas for source in area.sources
    }
    selected: set[ImpactArea] = set()
    unregistered: list[str] = []
    for raw_path in paths:
        path = PurePosixPath(raw_path.strip()).as_posix()
        if not path or path == ".":
            continue
        area = source_to_area.get(path)
        if area is not None:
            selected.add(area)
            continue
        if any(
            path.startswith(watched.path + "/") and path.endswith(watched.suffix)
            for watched in manifest.roots
        ):
            unregistered.append(path)
    if unregistered and reject_unregistered:
        raise ValueError(
            f"changed safety sources are unregistered: {sorted(unregistered)}"
        )
    return {
        "areas": sorted(area.identifier for area in selected),
        "claims": sorted({claim for area in selected for claim in area.claims}),
        "stages": [
            stage
            for stage in REQUIRED_STAGES
            if any(stage in area.stages for area in selected)
        ],
        "unregistered": sorted(unregistered),
    }


def validate_receipt_contents(receipt: object) -> None:
    if (
        not isinstance(receipt, dict)
        or type(receipt.get("schema")) is not int
        or receipt["schema"] != RECEIPT_SCHEMA
    ):
        raise ValueError(f"proof receipt must use schema {RECEIPT_SCHEMA}")
    if receipt.get("required_stages") != list(REQUIRED_STAGES):
        raise ValueError("proof receipt required stages do not match the impact policy")

    stages = receipt.get("stages")
    if not isinstance(stages, list) or not all(
        isinstance(stage, dict)
        and isinstance(stage.get("id"), str)
        and isinstance(stage.get("status"), str)
        for stage in stages
    ):
        raise ValueError("proof receipt stages must be typed records")
    stage_ids = [stage["id"] for stage in stages]
    if stage_ids != list(REQUIRED_STAGES):
        raise ValueError(
            "proof receipt stages must contain every required stage exactly once in order"
        )
    if any(stage["status"] != "pass" for stage in stages):
        raise ValueError("proof receipt contains a non-passing stage")

    claims = receipt.get("claims")
    if not isinstance(claims, list) or not claims:
        raise ValueError("proof receipt claims must be a non-empty list")
    derived_complete = (
        stage_ids == list(REQUIRED_STAGES)
        and all(stage["status"] == "pass" for stage in stages)
        and bool(claims)
    )
    if receipt.get("complete") is not derived_complete or not derived_complete:
        raise ValueError("proof receipt completion flag does not match its contents")


def check_receipt(receipt_path: Path, repo_root: Path = ROOT) -> None:
    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
    validate_receipt_contents(receipt)
    if receipt.get("source_fingerprint") != source_fingerprint(repo_root):
        raise ValueError("proof receipt source fingerprint is stale")
    if repo_root.resolve() != ROOT.resolve():
        raise ValueError("claim report regeneration requires the repository root")
    from check_claim_manifest import CLAIM_REPORT_SCHEMA, build_claim_report

    expected_report = build_claim_report()
    expected_claims = expected_report.get("claims")
    summary_statuses = (
        "implementation-proved",
        "refinement-tested",
        "partial",
        "assumed",
    )
    expected_summary = {
        status: sum(
            isinstance(claim, dict) and claim.get("status") == status
            for claim in expected_claims
        )
        for status in summary_statuses
    }
    if receipt.get("claim_report_schema") != CLAIM_REPORT_SCHEMA:
        raise ValueError("proof receipt claim report schema is stale")
    if receipt.get("claims") != expected_claims:
        raise ValueError("proof receipt claims do not match deterministic claim evidence")
    if receipt.get("claim_summary") != expected_summary:
        raise ValueError("proof receipt claim summary does not match deterministic claim evidence")
    if "claim_report_error" in receipt:
        raise ValueError("completed proof receipt contains a claim report error")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", nargs="*", help="changed paths to classify")
    parser.add_argument("--receipt", type=Path, help="validate a completed proof receipt")
    args = parser.parse_args()
    manifest = load_manifest()
    impact = classify_paths(args.paths, manifest)
    if args.receipt is not None:
        check_receipt(args.receipt)
    print(json.dumps(impact, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
