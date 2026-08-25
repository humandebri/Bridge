#!/usr/bin/env python3
"""Select the one trusted proof-source profile matching the checkout exactly."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
POLICY = ROOT / "scripts" / "trusted_proof_profiles.tsv"
PROFILE_ID = re.compile(r"[a-z][a-z0-9-]*")
DIGEST = re.compile(r"[0-9a-f]{64}")
VERIFICATION_EXCLUDED_DIRECTORIES = (
    ("output",),
    ("lean", ".lake"),
    ("smt", "out"),
    ("smt", "cache"),
    ("halmos", ".venv"),
    ("certora", ".venv"),
)
EXCLUDED_FILENAMES = frozenset({".DS_Store"})
SOURCE_GLOBS = (
    "contracts/test/halmos/**/*.sol",
)


@dataclass(frozen=True)
class TrustedProofProfile:
    identifier: str
    mode: str
    sources: dict[str, str]


def parse_policy(text: str) -> dict[str, TrustedProofProfile]:
    lines = [line for line in text.splitlines() if line]
    if not lines or lines[0].split("\t") != ["schema", "1", "-", "-"]:
        raise ValueError("trusted proof profile policy must start with schema 1")
    modes: dict[str, str] = {}
    sources: dict[str, dict[str, str]] = {}
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 4 or not all(fields):
            raise ValueError(f"invalid trusted proof profile row: {number}")
        kind, profile_id, value, digest = fields
        if PROFILE_ID.fullmatch(profile_id) is None:
            raise ValueError(f"invalid trusted proof profile ID: {number}")
        if kind == "profile":
            if value not in {"current", "hardening"} or digest != "-" or profile_id in modes:
                raise ValueError(f"invalid or duplicate trusted proof profile: {number}")
            modes[profile_id] = value
            sources[profile_id] = {}
        elif kind == "source":
            if profile_id not in modes or not value or value.startswith("/") or ".." in Path(value).parts:
                raise ValueError(f"invalid trusted proof source path: {number}")
            if DIGEST.fullmatch(digest) is None or value in sources[profile_id]:
                raise ValueError(f"invalid or duplicate trusted proof source: {number}")
            sources[profile_id][value] = digest
        else:
            raise ValueError(f"unknown trusted proof profile row: {number}")
    if set(modes) != {"current-main", "security-hardening-v1"}:
        raise ValueError("trusted proof profiles must be exactly current-main and security-hardening-v1")
    if any(not sources[profile_id] for profile_id in modes):
        raise ValueError("trusted proof profile cannot be empty")
    return {
        profile_id: TrustedProofProfile(profile_id, mode, sources[profile_id])
        for profile_id, mode in modes.items()
    }


def discover_sources(root: Path) -> set[str]:
    verification = root / "verification"
    discovered = {
        path.relative_to(root).as_posix()
        for path in verification.rglob("*")
        if path.is_file()
        and path.name not in EXCLUDED_FILENAMES
        and not any(
            verification.joinpath(*parts) in path.parents
            for parts in VERIFICATION_EXCLUDED_DIRECTORIES
        )
    }
    for pattern in SOURCE_GLOBS:
        discovered.update(
            path.relative_to(root).as_posix()
            for path in root.glob(pattern)
            if path.is_file()
        )
    return discovered


def matching_profiles(
    root: Path, profiles: dict[str, TrustedProofProfile]
) -> list[TrustedProofProfile]:
    discovered = discover_sources(root)
    matches: list[TrustedProofProfile] = []
    for profile in profiles.values():
        if set(profile.sources) != discovered:
            continue
        if all(
            hashlib.sha256((root / relative).read_bytes()).hexdigest() == digest
            for relative, digest in profile.sources.items()
        ):
            matches.append(profile)
    return matches


def select_profile(
    root: Path = ROOT, policy_path: Path = POLICY
) -> TrustedProofProfile:
    profiles = parse_policy(policy_path.read_text(encoding="utf-8"))
    matches = matching_profiles(root, profiles)
    if len(matches) != 1:
        raise ValueError(
            "checkout must match exactly one trusted proof profile: "
            f"matched={[profile.identifier for profile in matches]}"
        )
    return matches[0]


def require_profile(identifier: str, root: Path = ROOT) -> TrustedProofProfile:
    profile = select_profile(root)
    if profile.identifier != identifier:
        raise ValueError(
            f"trusted proof profile differs: expected={identifier} actual={profile.identifier}"
        )
    return profile


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--print", action="store_true", required=True)
    parser.parse_args()
    print(select_profile().identifier)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
