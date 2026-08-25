"""Deterministic source fingerprint shared by proof and claim gates."""

from __future__ import annotations

import argparse
import hashlib
import json
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
    ".node-version",
    ".gitmodules",
    "Cargo.lock",
    "Cargo.toml",
    "icp.yaml",
    "lean-toolchain",
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
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
FINGERPRINT_EXCLUDED_VERIFICATION_DIRS = (
    ("output",),
    ("lean", ".lake"),
    ("smt", "out"),
    ("smt", "cache"),
    ("halmos", ".venv"),
    ("certora", ".venv"),
)


def excluded_verification_path(path: Path, verification: Path) -> bool:
    relative = path.relative_to(verification)
    return any(part.startswith(".") for part in relative.parts) or any(
        verification.joinpath(*parts) in path.parents
        for parts in FINGERPRINT_EXCLUDED_VERIFICATION_DIRS
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
        and not excluded_verification_path(path, verification)
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
                and excluded_verification_path(path, verification)
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


def validate_fingerprint(value: object) -> dict[str, object]:
    if (
        not isinstance(value, dict)
        or value.get("algorithm") != "sha256"
        or not isinstance(value.get("digest"), str)
        or len(value["digest"]) != 64
        or any(character not in "0123456789abcdef" for character in value["digest"])
        or type(value.get("input_count")) is not int
        or value["input_count"] <= 0
    ):
        raise ValueError("proof source fingerprint has an invalid shape")
    return value


def load_fingerprint(path: Path) -> dict[str, object]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"proof source fingerprint is unreadable: {path}") from error
    return validate_fingerprint(value)


def write_fingerprint(path: Path, repo_root: Path = ROOT) -> dict[str, object]:
    value = source_fingerprint(repo_root)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")
    return value


def check_fingerprint(path: Path, repo_root: Path = ROOT) -> dict[str, object]:
    expected = load_fingerprint(path)
    actual = source_fingerprint(repo_root)
    if actual != expected:
        raise ValueError("proof inputs changed after the proof run started")
    return expected


def main() -> int:
    parser = argparse.ArgumentParser()
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--write", type=Path, metavar="PATH")
    action.add_argument("--check", type=Path, metavar="PATH")
    args = parser.parse_args()
    try:
        value = (
            write_fingerprint(args.write)
            if args.write is not None
            else check_fingerprint(args.check)
        )
    except ValueError as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps(value, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
