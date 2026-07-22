#!/usr/bin/env python3
"""Generate or verify Lean-owned protocol conformance vectors."""

from __future__ import annotations

import argparse
import difflib
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LEAN_ROOT = ROOT / "verification" / "lean"
VECTORS = ROOT / "verification" / "generated" / "protocol-vectors.json"


def generate() -> str:
    result = subprocess.run(
        ["lake", "exe", "bridge_spec_vectors"],
        cwd=LEAN_ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result.stdout


def matches_expected(current: str, generated: str, label: str) -> bool:
    if current == generated:
        return True
    diff = difflib.unified_diff(
        current.splitlines(),
        generated.splitlines(),
        fromfile=label,
        tofile="lean-generated:protocol-vectors.json",
        lineterm="",
    )
    sys.stderr.write("\n".join(diff) + "\n")
    return False


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    args = parser.parse_args()
    generated = generate()
    if args.update:
        VECTORS.parent.mkdir(parents=True, exist_ok=True)
        VECTORS.write_text(generated, encoding="utf-8")
        return 0
    if not VECTORS.exists():
        sys.stderr.write(f"missing protocol vectors: {VECTORS}\n")
        return 1
    current = VECTORS.read_text(encoding="utf-8")
    if not matches_expected(current, generated, str(VECTORS)):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
