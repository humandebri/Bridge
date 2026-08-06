"""Deterministic source fingerprint shared by proof and claim gates."""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FINGERPRINT_SOURCE_ROOTS = (
    ("canister", frozenset({".did", ".rs", ".toml"})),
    ("contracts", frozenset({".sol", ".toml"})),
    ("integration", frozenset({".ts"})),
    ("scripts", frozenset({"", ".mjs", ".py", ".sh"})),
    ("ui/src", frozenset({".ts", ".tsx"})),
    ("verification", frozenset({".json", ".lean", ".rs", ".sol", ".toml", ".tsv"})),
)
FINGERPRINT_CONFIG_FILES = (
    ".gitmodules",
    "Cargo.lock",
    "Cargo.toml",
    "package.json",
    "pnpm-lock.yaml",
    "rust-toolchain.toml",
    "ui/package.json",
    "ui/pnpm-lock.yaml",
    "ui/pnpm-workspace.yaml",
    "ui/tsconfig.app.json",
    "ui/tsconfig.json",
    "ui/tsconfig.node.json",
    "ui/vite.config.ts",
    "ui/vitest.config.ts",
)


def fingerprint_inputs(repo_root: Path = ROOT, manifest: Any | None = None) -> tuple[Path, ...]:
    if manifest is None:
        from check_proof_impact import load_manifest

        manifest = load_manifest(repo_root)
    paths = {repo_root / source for area in manifest.areas for source in area.sources}
    verification = repo_root / "verification"
    paths.update(
        path
        for path in verification.rglob("*")
        if path.is_file()
        and verification / "output" not in path.parents
        and ".lake" not in path.parts
    )
    for relative_root, suffixes in FINGERPRINT_SOURCE_ROOTS:
        source_root = repo_root / relative_root
        if not source_root.is_dir():
            raise ValueError(f"missing proof fingerprint root: {relative_root}")
        paths.update(
            path
            for path in source_root.rglob("*")
            if path.is_file()
            and path.suffix in suffixes
            and not (
                relative_root == "verification"
                and (
                    "output" in path.relative_to(source_root).parts
                    or any(part.startswith(".") for part in path.relative_to(source_root).parts)
                )
            )
        )
    patches = repo_root / "ui" / "patches"
    if patches.is_dir():
        paths.update(path for path in patches.rglob("*") if path.is_file())
    paths.update(repo_root / relative for relative in FINGERPRINT_CONFIG_FILES)
    missing = [path for path in paths if not path.is_file()]
    if missing:
        raise ValueError(
            "missing proof fingerprint inputs: "
            + ", ".join(path.relative_to(repo_root).as_posix() for path in sorted(missing))
        )
    return tuple(sorted(paths))


def source_fingerprint(repo_root: Path = ROOT, manifest: Any | None = None) -> dict[str, object]:
    digest = hashlib.sha256()
    inputs = fingerprint_inputs(repo_root, manifest)
    for path in inputs:
        relative = path.relative_to(repo_root).as_posix().encode()
        content = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return {"algorithm": "sha256", "digest": digest.hexdigest(), "input_count": len(inputs)}
