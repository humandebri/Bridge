#!/usr/bin/env python3
"""Classify changed repository paths into CI areas."""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import PurePosixPath


AREAS = ("rust", "contracts", "proofs", "ui", "real", "icp")


def _matches(path: str, prefixes: tuple[str, ...], exact: tuple[str, ...] = ()) -> bool:
    return path in exact or path.startswith(prefixes)


def _enable_all(result: dict[str, bool]) -> None:
    for area in AREAS:
        result[area] = True


def _is_documentation(path: str) -> bool:
    name = PurePosixPath(path).name
    return path.endswith(".md") or name.startswith("LICENSE") or name == ".gitignore"


def classify(paths: list[str]) -> dict[str, bool]:
    result = {area: False for area in AREAS}
    for raw_path in paths:
        path = PurePosixPath(raw_path.strip()).as_posix()
        if not path or path == ".":
            continue
        if _is_documentation(path):
            continue

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
        if _matches(path, ("tools/",)):
            result["rust"] = True
            classified = True
        if _matches(path, ("canister/", "integration/")):
            result["rust"] = True
            classified = True
        if _matches(path, ("contracts/",)):
            result["contracts"] = True
            classified = True
        if _matches(path, ("verification/",)) or path in {
            "canister/bridge-core/src/kernel.rs",
            "canister/bridge-core/tests/protocol_vectors.rs",
            "contracts/test/ProtocolVectors.t.sol",
            "ui/src/lib/protocol-vectors.test.ts",
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
    args = parser.parse_args()

    paths = list(args.paths)
    if not sys.stdin.isatty():
        data = sys.stdin.buffer.read()
        separator = b"\0" if args.null else b"\n"
        paths.extend(part.decode("utf-8") for part in data.split(separator) if part)

    result = classify(paths)
    lines = [f"{area}={'true' if enabled else 'false'}" for area, enabled in result.items()]
    lines.append(f"any={'true' if any(result.values()) else 'false'}")
    output_path = args.github_output or os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as output:
            output.write("\n".join(lines) + "\n")
    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
