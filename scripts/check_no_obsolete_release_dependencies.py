#!/usr/bin/env python3
"""Reject obsolete assets reachable from active proof or release entrypoints."""

from __future__ import annotations

import argparse
import os
import re
from pathlib import Path
import subprocess
import sys


FORBIDDEN_PATHS = frozenset(
    {
        "scripts/plan007/capture-obsolete-pause-evidence.mjs",
        "deployments/sepolia-staging/obsolete-replacement-policy.json",
        "deployments/sepolia-staging/obsolete-pause-capture.template.json",
        "deployments/sepolia-staging/evidence/archive/dbedb941/artifacts/obsolete-pause-evidence.json",
    }
)
SCRIPT_SUFFIXES = frozenset({".sh", ".py", ".mjs"})
CANONICAL_REFERENCE = re.compile(
    r"(?P<path>(?:scripts|deployments)/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*)"
)
SAME_DIRECTORY_SCRIPT = re.compile(
    r"(?:\)|SCRIPT_DIR\}?)/(?P<path>[A-Za-z0-9_.-]+\.(?:sh|py|mjs))(?=[\"'\s])"
)
REPLACEMENT_OPTIONAL_TEST_HELPER = (
    "scripts/plan007/test_staging_canister_upgrade.py"
)


def policy_files(root: Path) -> list[Path]:
    paths = [
        root / "scripts/ci-local.sh",
        root / "scripts/check_claim_test_manifest.py",
        root / "verification/proof-impact.tsv",
        root / "docs/runbooks/operations.md",
    ]
    return [path for path in paths if path.is_file()]


def dependency_entrypoints(root: Path) -> list[Path]:
    paths = [root / "scripts/ci-local.sh"]
    paths += sorted((root / "scripts").glob("production*.sh"))
    paths += sorted((root / "scripts/mainnet").glob("*.sh"))
    return [path for path in paths if path.is_file()]


def relative_path(root: Path, path: Path) -> str:
    return path.resolve().relative_to(root.resolve()).as_posix()


def is_test_helper(path: Path) -> bool:
    return path.name.startswith("test") or any(part.startswith("test") for part in path.parts)


def script_references(root: Path, path: Path, source: str) -> set[Path]:
    references: set[Path] = set()
    for match in CANONICAL_REFERENCE.finditer(source):
        candidate = root / match.group("path")
        if candidate.suffix in SCRIPT_SUFFIXES:
            references.add(candidate)
    for match in SAME_DIRECTORY_SCRIPT.finditer(source):
        references.add(path.parent / match.group("path"))
    return references


def replacement_layout_is_active(root: Path) -> bool:
    """Return true only when the trusted classifier accepts replacement layout."""

    classifier = root / "scripts/trusted_staging_layout.py"
    if not classifier.is_file() or classifier.is_symlink():
        return False
    command = [sys.executable, str(classifier), "--root", str(root)]
    candidate_scripts = os.environ.get("BRIDGE_CANDIDATE_SCRIPTS")
    if candidate_scripts:
        command.extend(["--candidate-scripts", candidate_scripts])
    try:
        result = subprocess.run(
            command,
            cwd=root,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return False
    return result.returncode == 0 and result.stdout.strip() == "replacement"


def optional_missing_helper(root: Path, reference: Path) -> bool:
    try:
        relative = reference.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        return False
    return (
        relative == REPLACEMENT_OPTIONAL_TEST_HELPER
        and replacement_layout_is_active(root)
    )


def dependency_closure(root: Path) -> tuple[set[Path], list[str]]:
    pending = dependency_entrypoints(root)
    visited: set[Path] = set()
    missing: list[str] = []
    while pending:
        path = pending.pop().resolve()
        if path in visited:
            continue
        visited.add(path)
        if path == (root / "scripts/check_no_obsolete_release_dependencies.py").resolve():
            continue
        source = path.read_text(encoding="utf-8")
        for reference in script_references(root, path, source):
            reference = reference.resolve()
            try:
                reference.relative_to(root.resolve())
            except ValueError:
                missing.append(f"{relative_path(root, path)}: outside-root helper {reference}")
                continue
            if not reference.is_file():
                if optional_missing_helper(root, reference):
                    continue
                missing.append(
                    f"{relative_path(root, path)}: missing helper "
                    f"{reference.relative_to(root.resolve()).as_posix()}"
                )
                continue
            if is_test_helper(reference):
                continue
            pending.append(reference)
    return visited, missing


def violations(root: Path) -> list[str]:
    closure, found = dependency_closure(root)
    active = closure | {path.resolve() for path in policy_files(root)}
    checker = (root / "scripts/check_no_obsolete_release_dependencies.py").resolve()
    for path in sorted(active):
        if path == checker:
            continue
        source = path.read_text(encoding="utf-8")
        for forbidden in sorted(FORBIDDEN_PATHS):
            if forbidden in source:
                found.append(f"{relative_path(root, path)}: {forbidden}")
    return sorted(set(found))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    found = violations(args.root.resolve())
    if found:
        raise SystemExit("obsolete active release dependency:\n" + "\n".join(found))
    print("obsolete active release dependency guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
