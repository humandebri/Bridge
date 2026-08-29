#!/usr/bin/env python3
"""Classify the staging lifecycle layout from trusted policy code."""

from __future__ import annotations

import argparse
import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
UPGRADE_SCRIPTS = (
    "plan007/staging_canister_upgrade.py",
    "plan007/staging-canister-upgrade.sh",
    "plan007/test_staging_canister_upgrade.py",
)


def _require_directory(path: Path, label: str) -> None:
    if path.is_symlink():
        raise ValueError(f"staging lifecycle {label} must not be symlinked")
    if not path.is_dir():
        raise ValueError(f"staging lifecycle {label} must be an existing directory")


def _regular_file_presence(root: Path, relative: str) -> bool:
    path = root / relative
    parents = path.relative_to(root).parents
    if path.is_symlink() or any(
        (root / parent).is_symlink() for parent in parents if parent != Path(".")
    ):
        raise ValueError(f"staging lifecycle path must not be symlinked: {relative}")
    if path.exists() and not path.is_file():
        raise ValueError(
            f"staging lifecycle path must be a regular file or absent: {relative}"
        )
    return path.is_file()


def classify_layout(root: Path, candidate_scripts: Path | None) -> str:
    _require_directory(root, "repository root")
    script_root = candidate_scripts if candidate_scripts is not None else root / "scripts"
    _require_directory(script_root, "script root")
    policy_prefix = "deployments/sepolia-staging"
    canonical = _regular_file_presence(
        root, f"{policy_prefix}/staging-bridge-upgrade-policy.json"
    )
    replacement = (
        _regular_file_presence(root, f"{policy_prefix}/legacy-stack-binding.json"),
        _regular_file_presence(root, f"{policy_prefix}/fresh-stack.template.json"),
    )
    obsolete = _regular_file_presence(
        root, f"{policy_prefix}/v33-to-v34-upgrade-policy.json"
    )
    upgrade_scripts = tuple(
        _regular_file_presence(script_root, relative) for relative in UPGRADE_SCRIPTS
    )

    if obsolete:
        raise ValueError("obsolete staging upgrade policy must be absent")
    if canonical and all(upgrade_scripts) and not any(replacement):
        return "upgrade"
    if not canonical and not any(upgrade_scripts) and all(replacement):
        return "replacement"
    raise ValueError(
        "staging lifecycle layout is incomplete or mixed: "
        f"canonical={canonical}, upgrade_scripts={upgrade_scripts}, replacement={replacement}"
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--candidate-scripts", type=Path)
    args = parser.parse_args()
    candidate_scripts = args.candidate_scripts
    if candidate_scripts is None and os.environ.get("BRIDGE_CANDIDATE_SCRIPTS"):
        candidate_scripts = Path(os.environ["BRIDGE_CANDIDATE_SCRIPTS"])

    try:
        print(classify_layout(args.root, candidate_scripts))
    except ValueError as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
