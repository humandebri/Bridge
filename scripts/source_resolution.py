"""Resolve candidate-owned source paths that the trusted gate masks."""

from __future__ import annotations

import os
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CANDIDATE_SCRIPTS = os.environ.get("BRIDGE_CANDIDATE_SCRIPTS")


def source_path(relative: str) -> Path:
    if CANDIDATE_SCRIPTS and relative.startswith("scripts/"):
        return Path(CANDIDATE_SCRIPTS) / relative.removeprefix("scripts/")
    return ROOT / relative


def is_inside_source_roots(path: Path) -> bool:
    """Return whether a resolved source path lives in the repo or candidate scripts root."""
    resolved = path.resolve()
    roots = [ROOT.resolve()]
    if CANDIDATE_SCRIPTS:
        roots.append(Path(CANDIDATE_SCRIPTS).resolve())
    return any(root in resolved.parents for root in roots)