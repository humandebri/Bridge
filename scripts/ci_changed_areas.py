#!/usr/bin/env python3
"""Classify changed repository paths into CI areas."""

from __future__ import annotations

import argparse
import csv
import json
import os
import re
import subprocess
import sys
from functools import lru_cache
from pathlib import Path
from pathlib import PurePosixPath

from certora_fingerprint import certora_python_dependency_paths
from proof_fingerprint import CERTORA_ONLY_RELEASE_EXCLUSIONS


AREAS = ("rust", "contracts", "proofs", "ui", "real", "icp", "certora")
ROOT = Path(__file__).resolve().parents[1]
CERTORA_ADVISORY_EXACT_PATHS = CERTORA_ONLY_RELEASE_EXCLUSIONS
FULL_SHA = re.compile(r"[0-9a-f]{40}")

SENSITIVE_PREFIXES = (
    ".github/",
    ".icp/data/mappings/",
    "contracts/lib/",
    "contracts/test/",
    "deployments/",
    "integration/",
    "recipes/",
    "scripts/",
    "tools/",
    "ui/e2e",
    "ui/patches/",
    "ui/scripts/",
    "verification/",
)
SENSITIVE_EXACT_PATHS = frozenset(
    {
        ".gitmodules",
        "Cargo.lock",
        "Cargo.toml",
        "contracts/foundry.toml",
        "icp.yaml",
        "lean-toolchain",
        "package.json",
        "package-lock.json",
        "npm-shrinkwrap.json",
        "pnpm-lock.yaml",
        "pnpm-workspace.yaml",
        "rust-toolchain.toml",
        "ui/package.json",
        "ui/pnpm-lock.yaml",
        "ui/pnpm-workspace.yaml",
    }
)
SENSITIVE_FILENAMES = frozenset(
    {
        "Cargo.toml",
        "Dockerfile",
        "Makefile",
        "build.rs",
        "foundry.toml",
        "icp.yaml",
        "lean-toolchain",
        "package.json",
        "pnpm-lock.yaml",
        "pnpm-workspace.yaml",
        "rust-toolchain",
        "rust-toolchain.toml",
        "toolchain.toml",
    }
)
SAFE_SOURCE_PREFIXES = (
    "canister/",
    "contracts/abi/",
    "contracts/src/",
    "ui/public/",
    "ui/src/",
)


@lru_cache(maxsize=1)
def _proof_owned_paths() -> frozenset[str]:
    manifest = ROOT / "verification" / "proof-impact.tsv"
    owned: set[str] = set()
    with manifest.open(encoding="utf-8", newline="") as source:
        for row in csv.reader(source, delimiter="\t"):
            if len(row) < 3 or row[0] != "area":
                continue
            for entry in row[2].split(";"):
                path = entry.partition("#")[0].strip()
                if path:
                    owned.add(path)
    return frozenset(owned)


def _matches(path: str, prefixes: tuple[str, ...], exact: tuple[str, ...] = ()) -> bool:
    return path in exact or path.startswith(prefixes)


def _enable_all(result: dict[str, bool]) -> None:
    for area in AREAS:
        result[area] = True


def _is_documentation(path: str) -> bool:
    name = PurePosixPath(path).name
    return (
        path.startswith("docs/")
        or path in {"README.md", "AGENTS.md", ".gitignore"}
        or ("/" not in path and name.startswith("LICENSE"))
    )


def _is_test_path(path: str) -> bool:
    parts = PurePosixPath(path).parts
    name = PurePosixPath(path).name
    return (
        any(
            part in {"test", "tests", "benches", "fixtures", "snapshots", "__tests__"}
            or part.startswith("e2e")
            for part in parts
        )
        or any(
            marker in name
            for marker in (".test.", ".spec.", ".e2e.", "_test.", "_tests.")
        )
        or name.startswith("test_")
    )


def raw_diff_has_gitlink(raw_diff: bytes) -> bool:
    """Return whether a raw git diff changes a 160000-mode entry."""
    for record in raw_diff.split(b"\0"):
        if not record.startswith(b":"):
            continue
        fields = record.split()
        if len(fields) >= 2 and (
            fields[0][1:] == b"160000" or fields[1] == b"160000"
        ):
            return True
    return False


def gitlink_changed(base_sha: str, head_sha: str, root: Path = ROOT) -> bool:
    """Inspect trusted Git metadata so a safe-looking submodule path cannot bypass review."""
    if FULL_SHA.fullmatch(base_sha) is None or FULL_SHA.fullmatch(head_sha) is None:
        raise ValueError("base and head must be lowercase full Git SHAs")
    result = subprocess.run(
        [
            "git",
            "-C",
            str(root),
            "diff",
            "--raw",
            "--no-abbrev",
            "-z",
            base_sha,
            head_sha,
        ],
        check=True,
        capture_output=True,
    )
    return raw_diff_has_gitlink(result.stdout)


def review_required(paths: list[str]) -> bool:
    """Require an exact-head review when a PR changes its validation boundary."""
    for raw_path in paths:
        path = PurePosixPath(raw_path.strip()).as_posix()
        if not path or path == ".":
            continue
        name = PurePosixPath(path).name
        if (
            path in SENSITIVE_EXACT_PATHS
            or path.startswith(SENSITIVE_PREFIXES)
            or _is_test_path(path)
            or name in SENSITIVE_FILENAMES
            or name.endswith((".lock", ".lockb"))
            or name.endswith((".config.js", ".config.mjs", ".config.ts"))
        ):
            return True
        if _is_documentation(path):
            continue
        if not path.startswith(SAFE_SOURCE_PREFIXES):
            return True
    return False


def _is_certora_advisory_only(path: str) -> bool:
    return path in CERTORA_ADVISORY_EXACT_PATHS or path.startswith(
        "verification/certora/"
    )


@lru_cache(maxsize=1)
def _certora_python_dependencies() -> frozenset[str]:
    return certora_python_dependency_paths(ROOT)


def classify(paths: list[str]) -> dict[str, bool]:
    result = {area: False for area in AREAS}
    for raw_path in paths:
        path = PurePosixPath(raw_path.strip()).as_posix()
        if not path or path == ".":
            continue
        if _is_documentation(path):
            continue
        if _is_certora_advisory_only(path):
            result["certora"] = True
            continue
        if path in _certora_python_dependencies():
            result["certora"] = True

        infrastructure = _matches(
            path,
            (".github/", "scripts/"),
            (
                ".gitmodules",
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                "package.json",
                "pnpm-lock.yaml",
            ),
        )
        if infrastructure:
            _enable_all(result)
            continue
        if _matches(path, ("deployments/", ".icp/data/mappings/")):
            _enable_all(result)
            continue

        classified = False
        if path in _proof_owned_paths():
            result["proofs"] = True
            classified = True
        if _matches(path, ("tools/",)):
            result["rust"] = True
            classified = True
        if _matches(path, ("canister/", "integration/")):
            result["rust"] = True
            classified = True
        if _matches(path, ("contracts/",)):
            result["contracts"] = True
            classified = True
        if _matches(path, ("verification/", "canister/", "contracts/src/")) or path in {
            "canister/bridge-core/tests/protocol_vectors.rs",
            "contracts/test/ProtocolVectors.t.sol",
            "ui/src/lib/pending-confirmations.ts",
            "ui/src/lib/protocol-vectors.test.ts",
            "ui/src/lib/withdrawal-confirmation-state.ts",
        }:
            result["proofs"] = True
            classified = True
        if _matches(path, ("ui/",)) or path in {
            "canister/bridge-canister/bridge.did",
            "canister/mock-external/mock.did",
        } or _matches(path, ("contracts/abi/", "contracts/src/")):
            result["ui"] = True
            classified = True
        if _matches(
            path,
            (
                "canister/",
                "integration/",
                "contracts/src/",
                "contracts/abi/",
                "ui/e2e-real/",
                "ui/src/features/bridge/",
                "ui/src/features/wallet/",
                "ui/src/lib/",
                "ui/src/lib/evm/",
                "ui/src/lib/ic/",
            ),
            (
                "ui/playwright.real.config.ts",
                "ui/vite.real.config.ts",
                "ui/scripts/download-ledger-artifacts.mjs",
                "ui/src/routes/history.tsx",
                "ui/src/routes/index.tsx",
            ),
        ):
            result["real"] = True
            classified = True
        if _matches(path, ("canister/", "recipes/"), ("icp.yaml",)):
            result["icp"] = True
            classified = True
        if not classified:
            _enable_all(result)

    return result


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("paths", nargs="*")
    parser.add_argument("--null", action="store_true", help="read NUL-delimited paths from stdin")
    parser.add_argument("--github-output", help="append key=value outputs to this file")
    parser.add_argument("--base-sha")
    parser.add_argument("--head-sha")
    args = parser.parse_args()
    if bool(args.base_sha) != bool(args.head_sha):
        parser.error("--base-sha and --head-sha must be provided together")

    paths = list(args.paths)
    if not sys.stdin.isatty():
        data = sys.stdin.buffer.read()
        separator = b"\0" if args.null else b"\n"
        paths.extend(part.decode("utf-8") for part in data.split(separator) if part)

    result = classify(paths)
    lines = [f"{area}={'true' if enabled else 'false'}" for area, enabled in result.items()]
    lines.append(f"any={'true' if any(result.values()) else 'false'}")
    requires_review = review_required(paths)
    if args.base_sha:
        requires_review = requires_review or gitlink_changed(args.base_sha, args.head_sha)
    lines.append(f"review_required={'true' if requires_review else 'false'}")
    enabled_areas = [area for area, enabled in result.items() if enabled]
    # GitHub Actions rejects a dynamically expanded matrix with zero values.
    # Keep `any=false` as the authoritative skip signal and use a sentinel so
    # the matrix can still be evaluated; the guarded jobs never execute it.
    workflow_areas = enabled_areas or ["none"]
    lines.append(
        "matrix="
        + json.dumps(
            workflow_areas,
            separators=(",", ":"),
        )
    )
    output_path = args.github_output or os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as output:
            output.write("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
