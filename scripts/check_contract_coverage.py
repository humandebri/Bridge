#!/usr/bin/env python3
"""Fail closed when Foundry LCOV coverage is missing or below policy."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path


THRESHOLDS_BPS = {"lines": 7600, "branches": 7400, "functions": 6800}
SUMMARY_KEYS = {
    "lines": ("LH", "LF"),
    "branches": ("BRH", "BRF"),
    "functions": ("FNH", "FNF"),
}


@dataclass(frozen=True)
class Coverage:
    hit: int
    total: int

    @property
    def basis_points(self) -> int:
        return self.hit * 10_000 // self.total


def parse_lcov(text: str) -> dict[str, Coverage]:
    records: list[dict[str, int]] = []
    current: dict[str, int] = {}
    saw_content = False
    allowed = {key for pair in SUMMARY_KEYS.values() for key in pair}

    for number, line in enumerate(text.splitlines(), start=1):
        if not line:
            continue
        saw_content = True
        if line == "end_of_record":
            if not current:
                raise ValueError(f"empty LCOV record ending at line {number}")
            records.append(current)
            current = {}
            continue
        key, separator, value = line.partition(":")
        if key not in allowed:
            continue
        if not separator or key in current:
            raise ValueError(f"invalid or duplicate LCOV summary at line {number}")
        try:
            parsed = int(value)
        except ValueError as error:
            raise ValueError(f"invalid LCOV integer at line {number}") from error
        if parsed < 0:
            raise ValueError(f"negative LCOV value at line {number}")
        current[key] = parsed

    if current:
        raise ValueError("LCOV record is missing end_of_record")
    if not saw_content or not records:
        raise ValueError("LCOV contains no records")

    result: dict[str, Coverage] = {}
    for metric, (hit_key, total_key) in SUMMARY_KEYS.items():
        for record in records:
            if hit_key not in record or total_key not in record:
                raise ValueError(f"LCOV record is missing {hit_key}/{total_key}")
        hit = sum(record[hit_key] for record in records)
        total = sum(record[total_key] for record in records)
        if total == 0:
            raise ValueError(f"LCOV {metric} denominator is zero")
        if hit > total:
            raise ValueError(f"LCOV {metric} hits exceed total")
        result[metric] = Coverage(hit, total)
    return result


def enforce_thresholds(coverage: dict[str, Coverage]) -> None:
    for metric, threshold in THRESHOLDS_BPS.items():
        value = coverage[metric]
        if value.hit * 10_000 < value.total * threshold:
            raise ValueError(
                f"{metric} coverage {value.basis_points / 100:.2f}% is below "
                f"{threshold / 100:.2f}%"
            )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("lcov", type=Path)
    args = parser.parse_args()
    try:
        coverage = parse_lcov(args.lcov.read_text(encoding="utf-8"))
        enforce_thresholds(coverage)
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(str(error))
    print(
        "contract coverage passed: "
        + ", ".join(
            f"{metric}={value.basis_points / 100:.2f}%"
            for metric, value in coverage.items()
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
