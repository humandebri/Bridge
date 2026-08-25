"""Resolve candidate-owned source paths that the trusted gate masks."""

from __future__ import annotations

import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CANDIDATE_SCRIPTS = os.environ.get("BRIDGE_CANDIDATE_SCRIPTS")


def source_path(relative: str, repo_root: Path = ROOT) -> Path:
    if CANDIDATE_SCRIPTS and relative.startswith("scripts/"):
        return Path(CANDIDATE_SCRIPTS) / relative.removeprefix("scripts/")
    return repo_root / relative


def source_root(relative: str, repo_root: Path = ROOT) -> Path:
    if CANDIDATE_SCRIPTS and relative == "scripts":
        return Path(CANDIDATE_SCRIPTS)
    return repo_root / relative


def logical_source_path(path: Path, repo_root: Path = ROOT) -> Path:
    resolved = path.resolve()
    if CANDIDATE_SCRIPTS:
        candidate_root = Path(CANDIDATE_SCRIPTS).resolve()
        if resolved == candidate_root or resolved.is_relative_to(candidate_root):
            return Path("scripts") / resolved.relative_to(candidate_root)
    return resolved.relative_to(repo_root.resolve())


def is_inside_source_roots(path: Path) -> bool:
    """Return whether a resolved source path lives in the repo or candidate scripts root."""
    resolved = path.resolve()
    roots = [ROOT.resolve()]
    if CANDIDATE_SCRIPTS:
        roots.append(Path(CANDIDATE_SCRIPTS).resolve())
    return any(root in resolved.parents for root in roots)
