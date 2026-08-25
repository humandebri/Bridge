#!/usr/bin/env python3
"""Write the deterministic, machine-readable local proof-stage receipt."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from check_claim_manifest import CLAIM_REPORT_SCHEMA, REPORT, build_claim_report
from check_proof_impact import (
    RECEIPT_SCHEMA,
    REQUIRED_STAGES,
    release_summary_is_complete,
    source_fingerprint,
    summarize_claim_report,
)
from proof_fingerprint import load_fingerprint

REQUIRED = REQUIRED_STAGES


def current_claim_evidence(
    baseline: dict[str, object],
) -> tuple[list[dict[str, object]], list[dict[str, object]], dict[str, object]]:
    if not REPORT.is_file():
        raise ValueError("claim report is missing")
    try:
        report = json.loads(REPORT.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("claim report is unreadable") from error

    fingerprint_before = source_fingerprint()
    if fingerprint_before != baseline:
        raise ValueError("proof inputs changed after the proof run started")
    expected = build_claim_report()
    fingerprint_after = source_fingerprint()
    if fingerprint_after != baseline:
        raise ValueError("proof inputs changed while computing claim evidence")
    claims = report.get("claims")
    conditional_liveness = report.get("conditional_liveness")
    if report.get("schema") != CLAIM_REPORT_SCHEMA:
        raise ValueError("claim report schema does not match the current generator")
    if report.get("source_fingerprint") != fingerprint_before:
        raise ValueError("claim report source fingerprint is stale")
    if report != expected:
        raise ValueError("claim report does not match deterministic generator output")
    if not isinstance(claims, list) or not claims:
        raise ValueError("claim evidence must contain a non-empty claim list")
    if not all(isinstance(claim, dict) for claim in claims):
        raise ValueError("claim evidence contains a malformed claim")
    if not isinstance(conditional_liveness, list) or not all(
        isinstance(prop, dict) for prop in conditional_liveness
    ):
        raise ValueError("conditional liveness evidence is malformed")
    return claims, conditional_liveness, baseline


def main() -> int:
    if len(sys.argv) != 4:
        raise SystemExit(
            "usage: write_proof_receipt.py STAGE_TSV RECEIPT_JSON BASELINE_JSON"
        )
    stages_path, receipt_path, baseline_path = map(Path, sys.argv[1:])
    baseline = load_fingerprint(baseline_path)
    stages: dict[str, tuple[str, dict[str, object]]] = {}
    if stages_path.exists():
        for line in stages_path.read_text(encoding="utf-8").splitlines():
            try:
                stage, status, raw_fingerprint = line.split("\t")
                stage_fingerprint = json.loads(raw_fingerprint)
            except (ValueError, json.JSONDecodeError) as error:
                raise ValueError(f"invalid proof receipt stage: {line}") from error
            if stage not in REQUIRED or status not in {"pass", "fail"} or stage in stages:
                raise ValueError(f"invalid proof receipt stage: {line}")
            if stage_fingerprint != baseline:
                raise ValueError(f"proof stage fingerprint differs from baseline: {stage}")
            stages[stage] = (status, stage_fingerprint)
    claim_error: str | None = None
    try:
        claims, conditional_liveness, fingerprint = current_claim_evidence(baseline)
    except ValueError as error:
        claims = []
        conditional_liveness = []
        fingerprint = baseline
        claim_error = str(error)
    claim_summary = summarize_claim_report(claims, conditional_liveness)
    complete = (
        claim_error is None
        and tuple(stages) == REQUIRED
        and all(status == "pass" for status, _ in stages.values())
        and bool(claims)
        and release_summary_is_complete(claim_summary)
    )
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(
        json.dumps(
            {
                "schema": RECEIPT_SCHEMA,
                "claim_report_schema": CLAIM_REPORT_SCHEMA,
                "required_stages": list(REQUIRED),
                "stages": [
                    {
                        "id": stage,
                        "status": stages[stage][0],
                        "source_fingerprint": stages[stage][1],
                    }
                    for stage in stages
                ],
                "source_fingerprint": fingerprint,
                "claims": claims,
                "conditional_liveness": conditional_liveness,
                "claim_summary": claim_summary,
                "complete": complete,
                **({"claim_report_error": claim_error} if claim_error is not None else {}),
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    return 0 if claim_error is None else 1


if __name__ == "__main__":
    raise SystemExit(main())
