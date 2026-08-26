#!/usr/bin/env python3
"""Materialize only dependency manifests authenticated by the proof profile."""

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import shutil

from trusted_proof_profiles import POLICY as PROOF_POLICY
from trusted_proof_profiles import select_profile


ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "scripts" / "trusted_dependency_profiles.tsv"


def parse_policy(text: str) -> dict[str, dict[str, str]]:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != ["schema", "1", "-", "-"]:
        raise ValueError("trusted dependency policy must start with schema 1")
    profiles: dict[str, dict[str, str]] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 4:
            raise ValueError(f"invalid trusted dependency row: {number}")
        kind, profile, relative, digest = fields
        if kind == "profile":
            if relative != "-" or digest != "-" or profile in profiles:
                raise ValueError(f"invalid trusted dependency profile: {number}")
            profiles[profile] = {}
        elif kind == "source":
            path = Path(relative)
            if (
                profile not in profiles
                or path.is_absolute()
                or ".." in path.parts
                or len(digest) != 64
                or any(character not in "0123456789abcdef" for character in digest)
                or relative in profiles[profile]
            ):
                raise ValueError(f"invalid trusted dependency source: {number}")
            profiles[profile][relative] = digest
        else:
            raise ValueError(f"unknown trusted dependency row: {number}")
    if set(profiles) != {"security-hardening-v1"}:
        raise ValueError("security-hardening-v1 must be the only dependency profile")
    return profiles


def materialize(source: Path, destination: Path, policy_path: Path = POLICY) -> str:
    profile = select_profile(source, PROOF_POLICY).identifier
    expected = parse_policy(policy_path.read_text(encoding="utf-8"))[profile]
    if destination.exists():
        raise ValueError("trusted dependency destination already exists")
    for relative, digest in expected.items():
        source_path = source / relative
        if not source_path.is_file() or source_path.is_symlink():
            raise ValueError(f"trusted dependency source is missing or symlinked: {relative}")
        actual = hashlib.sha256(source_path.read_bytes()).hexdigest()
        if actual != digest:
            raise ValueError(f"trusted dependency digest mismatch: {relative}")
        destination_path = destination / relative
        destination_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source_path, destination_path)
    return profile


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    print(materialize(args.source.resolve(), args.destination.resolve()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
