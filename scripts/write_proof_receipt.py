#!/usr/bin/env python3
"""Write the deterministic, machine-readable local proof-stage receipt."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from check_claim_manifest import build_claim_report
from check_proof_impact import RECEIPT_SCHEMA, REQUIRED_STAGES, source_fingerprint

REQUIRED = REQUIRED_STAGES


def current_claim_evidence() -> tuple[list[dict[str, object]], dict[str, object]]:
    fingerprint_before = source_fingerprint()
    report = build_claim_report()
    fingerprint_after = source_fingerprint()
    if fingerprint_before != fingerprint_after:
        raise ValueError("proof inputs changed while computing claim evidence")
    claims = report.get("claims")
    if report.get("schema") != 1 or not isinstance(claims, list) or not claims:
        raise ValueError("claim evidence must contain a non-empty schema-1 claim list")
    if not all(isinstance(claim, dict) for claim in claims):
        raise ValueError("claim evidence contains a malformed claim")
    return claims, fingerprint_after


def main() -> int:
    if len(sys.argv) != 3:
        raise SystemExit("usage: write_proof_receipt.py STAGE_TSV RECEIPT_JSON")
    stages_path, receipt_path = map(Path, sys.argv[1:])
    stages: dict[str, str] = {}
    if stages_path.exists():
        for line in stages_path.read_text(encoding="utf-8").splitlines():
            stage, status = line.split("\t")
            if stage not in REQUIRED or status not in {"pass", "fail"} or stage in stages:
                raise ValueError(f"invalid proof receipt stage: {line}")
            stages[stage] = status
    claims, fingerprint = current_claim_evidence()
    complete = (
        tuple(stages) == REQUIRED
        and all(status == "pass" for status in stages.values())
        and bool(claims)
    )
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(
        json.dumps(
            {
                "schema": RECEIPT_SCHEMA,
                "required_stages": list(REQUIRED),
                "stages": [{"id": stage, "status": stages[stage]} for stage in stages],
                "source_fingerprint": fingerprint,
                "claims": claims,
                "claim_summary": {
                    status: sum(claim.get("status") == status for claim in claims)
                    for status in (
                        "implementation-proved",
                        "refinement-tested",
                        "partial",
                        "assumed",
                    )
                },
                "complete": complete,
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
