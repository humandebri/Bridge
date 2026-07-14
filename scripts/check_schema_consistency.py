#!/usr/bin/env python3
"""Reject stale stable-schema references in code, CI, and operator documentation."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def require_versions(path: str, pattern: str, expected: int, expected_count: int) -> None:
    text = (ROOT / path).read_text(encoding="utf-8")
    versions = [int(version) for version in re.findall(pattern, text)]
    if not versions:
        raise SystemExit(f"missing stable schema declaration in {path}")
    if len(versions) != expected_count:
        raise SystemExit(
            f"stable schema declaration count mismatch: {path} has {len(versions)}, "
            f"expected {expected_count}"
        )
    mismatches = [version for version in versions if version != expected]
    if mismatches:
        raise SystemExit(
            f"stable schema mismatch: {path} declares {mismatches}, Rust declares v{expected}"
        )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--print-version", action="store_true")
    args = parser.parse_args()

    storage = (ROOT / "canister/bridge-canister/src/storage.rs").read_text(
        encoding="utf-8"
    )
    match = re.search(r"pub const SCHEMA_VERSION: u16 = (\d+);", storage)
    if match is None:
        raise SystemExit("missing SCHEMA_VERSION declaration")
    version = int(match.group(1))
    checks = (
        ("README.md", r"(?:stable schema v|schema version )(\d+)", 2),
        ("verification/README.md", r"schema v(\d+)再オープン", 1),
        ("docs/canister-state-machine.md", r"(?:Stable schema v|schema v)(\d+)", 2),
        ("plan.md", r"(?:stable schema v|現行stable schemaはv)(\d+)", 2),
        ("ui/src/lib/runtime-validation.ts", r"config\.schema_version !== (\d+)", 1),
        ("integration/phase3.spec.ts", r"config\.schema_version\)\.toBe\((\d+)\)", 1),
    )
    for path, pattern, expected_count in checks:
        require_versions(path, pattern, version, expected_count)
    if args.print_version:
        print(version)


if __name__ == "__main__":
    main()
