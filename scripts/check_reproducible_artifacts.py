#!/usr/bin/env python3
"""Compare two fixed-source builds with each other and with a release bundle."""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path


ARTIFACTS = (
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
)
SHA256 = re.compile(r"^[0-9a-fA-F]{64}$")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def expected_hashes(manifest_path: Path) -> dict[str, str]:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    entries = manifest.get("artifacts")
    if not isinstance(entries, list):
        raise ValueError("release manifest artifacts are missing")
    result: dict[str, str] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        path = entry.get("path")
        value = entry.get("sha256")
        if path in ARTIFACTS:
            if path in result or not isinstance(value, str) or not SHA256.fullmatch(value):
                raise ValueError(f"invalid or duplicate release artifact: {path}")
            result[path] = value.lower()
    if set(result) != set(ARTIFACTS):
        raise ValueError("release manifest must bind every reproducible artifact")
    return result


def verify(bundle: Path, first: Path, second: Path) -> None:
    expected = expected_hashes(bundle / "release-manifest.json")
    for name in ARTIFACTS:
        paths = (bundle / name, first / name, second / name)
        if any(not path.is_file() or path.is_symlink() for path in paths):
            raise ValueError(f"reproducible artifact is missing or a symlink: {name}")
        hashes = tuple(digest(path) for path in paths)
        if len(set(hashes)) != 1:
            raise ValueError(f"independent builds differ for {name}")
        if hashes[0] != expected[name]:
            raise ValueError(f"rebuilt artifact differs from release manifest: {name}")


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(f"usage: {argv[0]} BUNDLE FIRST_BUILD SECOND_BUILD", file=sys.stderr)
        return 2
    try:
        verify(*(Path(value).resolve() for value in argv[1:]))
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(error, file=sys.stderr)
        return 1
    print("reproducible Wasm and contract creation/runtime hashes match the release manifest")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
