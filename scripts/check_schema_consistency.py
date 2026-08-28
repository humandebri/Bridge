#!/usr/bin/env python3
"""Reject stale stable-schema references in code, CI, and operator documentation."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

from source_resolution import ROOT, source_path


def require_versions(path: str, pattern: str, expected: int, expected_count: int) -> None:
    text = source_path(path).read_text(encoding="utf-8")
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

    storage = (ROOT / "canister/bridge-canister/src/storage/schema.rs").read_text(
        encoding="utf-8"
    )
    match = re.search(r"pub const SCHEMA_VERSION: u16 = (\d+);", storage)
    if match is None:
        raise SystemExit("missing SCHEMA_VERSION declaration")
    version = int(match.group(1))
    wire_match = re.search(r"const WIRE_VERSION: u8 = (\d+);", storage)
    if wire_match is None:
        raise SystemExit("missing WIRE_VERSION declaration")
    wire_version = int(wire_match.group(1))
    checks = (
        ("README.md", r"(?:stable schema v|schema version )(\d+)", 2),
        ("verification/README.md", r"schema v(\d+)再オープン", 1),
        ("docs/bridge-flow.md", r"SQLite schema v(\d+)", 1),
        ("docs/canister-state-machine.md", r"(?:Stable schema v|schema v)(\d+)", 2),
        (
            "docs/runbooks/operations.md",
            r"(?:stable schemaはv|schema v)(\d+)(?:、record wireはv\d+|またはwire v\d+)",
            2,
        ),
        (
            "deployments/sepolia-staging/evidence/README.md",
            r"Canister v(\d+) (?:upgrade|install)",
            1,
        ),
        ("docs/implementation-plan.md", r"(?:stable schema v|現行stable schemaはv)(\d+)", 2),
        (
            "canister/bridge-canister/src/storage/mod.rs",
            r"assert_eq!\(SCHEMA_VERSION, (\d+)\);",
            1,
        ),
        (
            "canister/bridge-canister/src/storage/mod.rs",
            r"INSERT INTO bridge_metadata VALUES \(1, (\d+), \d+\);",
            1,
        ),
        (
            "tools/bridge-profile/src/main.rs",
            r"CURRENT_STABLE_SCHEMA_VERSION: u16 = (\d+);",
            1,
        ),
        ("ui/src/lib/runtime-validation.ts", r"config\.schema_version !== (\d+)", 1),
        (
            "integration/phase3.spec.ts",
            r"(?:config|before|after)\.schema_version\)\.toBe\((\d+)\)",
            3,
        ),
        (
            "scripts/test_production_drivers.sh",
            r'"schema_version":(\d+),"expected_bridge_signer"',
            1,
        ),
        (
            "scripts/plan007/generate-local-e2e.mjs",
            r"CURRENT_STABLE_SCHEMA_VERSION = (\d+)",
            1,
        ),
        (
            "scripts/plan007/sepolia_e2e.py",
            r"CURRENT_STABLE_SCHEMA = (\d+)",
            1,
        ),
    )
    for path, pattern, expected_count in checks:
        require_versions(path, pattern, version, expected_count)
    wire_checks = (
        ("verification/README.md", r"wire v(\d+)", 1),
        ("docs/canister-state-machine.md", r"record wire version v(\d+)", 1),
        (
            "docs/runbooks/operations.md",
            r"(?:stable schemaはv\d+、record wireはv|schema v\d+またはwire v)(\d+)",
            2,
        ),
        ("docs/implementation-plan.md", r"record wire v(\d+)", 1),
        (
            "canister/bridge-canister/src/storage/mod.rs",
            r"INSERT INTO bridge_metadata VALUES \(1, \d+, (\d+)\);",
            1,
        ),
    )
    for path, pattern, expected_count in wire_checks:
        require_versions(path, pattern, wire_version, expected_count)
    if args.print_version:
        print(version)


if __name__ == "__main__":
    main()
