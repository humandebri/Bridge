#!/usr/bin/env python3
"""Bind the installed Halmos virtual environment to its locked inputs."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PROJECT = ROOT / "verification" / "halmos"
STAMP = PROJECT / ".venv" / "bridge-lock.json"
INPUTS = (PROJECT / "pyproject.toml", PROJECT / "uv.lock")


def lock_fingerprint() -> dict[str, object]:
    digest = hashlib.sha256()
    for path in INPUTS:
        if not path.is_file():
            raise ValueError(f"missing Halmos environment input: {path}")
        relative = path.relative_to(ROOT).as_posix().encode()
        content = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return {"schema": 1, "algorithm": "sha256", "digest": digest.hexdigest()}


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    expected = lock_fingerprint()
    if args.write:
        if not STAMP.parent.is_dir():
            raise ValueError("Halmos virtual environment is missing")
        STAMP.write_text(json.dumps(expected, sort_keys=True) + "\n", encoding="utf-8")
        return 0
    try:
        actual = json.loads(STAMP.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError("Halmos environment lock stamp is missing or malformed") from error
    if actual != expected:
        raise ValueError("Halmos environment does not match the current lock inputs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
