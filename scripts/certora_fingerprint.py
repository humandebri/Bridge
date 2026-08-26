#!/usr/bin/env python3
"""Fingerprint the complete input boundary of advisory Certora evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from proof_fingerprint import validate_fingerprint


ROOT = Path(__file__).resolve().parents[1]
CERTORA_EXACT_INPUTS = (
    ".github/workflows/certora-advisory.yml",
    ".gitmodules",
    "contracts/foundry.toml",
    "contracts/test/BridgeTimelock.t.sol",
    "scripts/certora_fingerprint.py",
    "scripts/certora_results.py",
    "scripts/check_certora_manifest.py",
    "scripts/install-certora-solc.sh",
    "scripts/proof_fingerprint.py",
    "scripts/run_certora_advisory.sh",
    "scripts/test_certora_fingerprint.py",
    "scripts/test_certora_manifest.py",
    "scripts/test_certora_results.py",
    "verification/assumptions.tsv",
    "verification/claims.tsv",
)
CERTORA_SOURCE_ROOTS = (
    ("contracts/src", frozenset({".sol"})),
    ("contracts/lib/openzeppelin-contracts/contracts", frozenset({".sol"})),
    ("verification/certora", None),
)
CERTORA_EXCLUDED_PARTS = frozenset({".venv", "__pycache__"})


def certora_fingerprint_inputs(repo_root: Path = ROOT) -> tuple[Path, ...]:
    paths = {repo_root / relative for relative in CERTORA_EXACT_INPUTS}
    for relative_root, suffixes in CERTORA_SOURCE_ROOTS:
        source_root = repo_root / relative_root
        if not source_root.is_dir():
            raise ValueError(f"missing Certora fingerprint root: {relative_root}")
        paths.update(
            path
            for path in source_root.rglob("*")
            if path.is_file()
            and not any(part in CERTORA_EXCLUDED_PARTS for part in path.parts)
            and (suffixes is None or path.suffix in suffixes)
        )
    missing = [path for path in paths if not path.is_file()]
    if missing:
        raise ValueError(
            "missing Certora fingerprint inputs: "
            + ", ".join(
                path.relative_to(repo_root).as_posix() for path in sorted(missing)
            )
        )
    return tuple(sorted(paths))


def certora_source_fingerprint(repo_root: Path = ROOT) -> dict[str, object]:
    digest = hashlib.sha256()
    inputs = certora_fingerprint_inputs(repo_root)
    for path in inputs:
        relative = path.relative_to(repo_root).as_posix().encode()
        content = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return {
        "algorithm": "sha256",
        "digest": digest.hexdigest(),
        "input_count": len(inputs),
        "scope": "certora-advisory-v1",
    }


def validate_certora_fingerprint(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != {
        "algorithm",
        "digest",
        "input_count",
        "scope",
    }:
        raise ValueError("Certora source fingerprint has an invalid shape")
    core = {key: value[key] for key in ("algorithm", "digest", "input_count")}
    validate_fingerprint(core)
    if value["scope"] != "certora-advisory-v1":
        raise ValueError("Certora source fingerprint has an invalid scope")
    return value


def write_fingerprint(path: Path, repo_root: Path = ROOT) -> dict[str, object]:
    value = certora_source_fingerprint(repo_root)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")
    return value


def check_fingerprint(path: Path, repo_root: Path = ROOT) -> dict[str, object]:
    expected = json.loads(path.read_text(encoding="utf-8"))
    validate_certora_fingerprint(expected)
    actual = certora_source_fingerprint(repo_root)
    if expected != actual:
        raise ValueError("Certora inputs changed after the advisory run started")
    return actual


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", type=Path)
    mode.add_argument("--check", type=Path)
    args = parser.parse_args()
    try:
        if args.write is not None:
            write_fingerprint(args.write)
        else:
            check_fingerprint(args.check)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
