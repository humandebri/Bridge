#!/usr/bin/env python3
"""Write the deterministic, machine-readable local proof-stage receipt."""

from __future__ import annotations

import json
import sys
from pathlib import Path

REQUIRED = (
    "lean",
    "lean-negative",
    "policy-vector-consumers",
    "refinement-gate",
    "known-answer-consumers",
    "smt-and-negative",
    "verus-and-negative",
)
ROOT = Path(__file__).resolve().parents[1]
CLAIM_REPORT = ROOT / "verification" / "output" / "claim-report.json"


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
    claim_report = (
        json.loads(CLAIM_REPORT.read_text(encoding="utf-8"))
        if CLAIM_REPORT.is_file()
        else {"schema": 1, "claims": []}
    )
    claims = claim_report.get("claims", [])
    complete = (
        tuple(stages) == REQUIRED
        and all(status == "pass" for status in stages.values())
        and bool(claims)
    )
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    receipt_path.write_text(
        json.dumps(
            {
                "schema": 2,
                "required_stages": list(REQUIRED),
                "stages": [{"id": stage, "status": stages[stage]} for stage in stages],
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
