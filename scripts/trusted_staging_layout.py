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


def classify_layout(root: Path, candidate_scripts: Path | None) -> str:
    policy_dir = root / "deployments" / "sepolia-staging"
    script_root = candidate_scripts if candidate_scripts is not None else root / "scripts"
    canonical = (policy_dir / "staging-bridge-upgrade-policy.json").is_file()
    replacement = (
        (policy_dir / "legacy-stack-binding.json").is_file(),
        (policy_dir / "fresh-stack.template.json").is_file(),
    )
    upgrade_scripts = tuple((script_root / relative).is_file() for relative in UPGRADE_SCRIPTS)

    if (policy_dir / "v33-to-v34-upgrade-policy.json").exists():
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
