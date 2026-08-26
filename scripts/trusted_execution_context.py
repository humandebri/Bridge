#!/usr/bin/env python3
"""Bind trusted CI execution to the exact candidate checkout HEAD."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[1]
FULL_SHA = re.compile(r"[0-9a-f]{40}")


def checkout_head(root: Path = ROOT) -> str:
    result = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "HEAD"],
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def require_trusted_execution_context(root: Path = ROOT) -> str | None:
    """Require an exact expected HEAD in CI; local validation needs no profile."""
    if os.environ.get("CI") != "true":
        return None
    expected = os.environ.get("BRIDGE_EXPECTED_HEAD_SHA", "")
    if FULL_SHA.fullmatch(expected) is None:
        raise ValueError("BRIDGE_EXPECTED_HEAD_SHA must be a lowercase full Git SHA")
    actual = checkout_head(root)
    if actual != expected:
        raise ValueError(
            f"trusted checkout HEAD differs: expected={expected} actual={actual}"
        )
    return actual


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", required=True)
    parser.parse_args()
    head = require_trusted_execution_context()
    print(head if head is not None else "local")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
