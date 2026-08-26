#!/usr/bin/env python3
"""Isolate the candidate PR's fixed dependency manifest and patch inputs."""

from __future__ import annotations

import argparse
import os
from pathlib import Path, PurePosixPath
import shutil


ROOT = Path(__file__).resolve().parents[1]
REQUIRED_MANIFESTS = (
    "package.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    "ui/package.json",
    "ui/pnpm-lock.yaml",
    "ui/pnpm-workspace.yaml",
)
MANIFEST_NAMES = frozenset({"package.json", "pnpm-lock.yaml", "pnpm-workspace.yaml"})
IGNORED_DISCOVERY_PARTS = frozenset({".git", "node_modules"})
PATCH_ROOT = PurePosixPath("ui/patches")


def _regular_file(source: Path, relative: str) -> None:
    path = source / relative
    parents = path.relative_to(source).parents
    if (
        not path.is_file()
        or path.is_symlink()
        or any((source / parent).is_symlink() for parent in parents if parent != Path("."))
    ):
        raise ValueError(f"candidate dependency input is missing or not regular: {relative}")


def _inside_nested_git_checkout(source: Path, path: Path) -> bool:
    for parent in path.relative_to(source).parents:
        if parent != Path(".") and (source / parent / ".git").exists():
            return True
    return False


def candidate_dependency_sources(source: Path) -> tuple[str, ...]:
    source = source.resolve()
    expected = set(REQUIRED_MANIFESTS)
    discovered = {
        path.relative_to(source).as_posix()
        for name in MANIFEST_NAMES
        for path in source.rglob(name)
        if IGNORED_DISCOVERY_PARTS.isdisjoint(path.relative_to(source).parts)
        and not _inside_nested_git_checkout(source, path)
    }
    unknown = discovered - expected
    missing = expected - discovered
    if unknown:
        raise ValueError(f"unknown dependency manifest placement: {sorted(unknown)}")
    if missing:
        raise ValueError(f"required dependency manifests are missing: {sorted(missing)}")

    inputs = list(REQUIRED_MANIFESTS)
    for relative in REQUIRED_MANIFESTS:
        _regular_file(source, relative)

    patch_root = source / PATCH_ROOT
    if not patch_root.is_dir() or patch_root.is_symlink():
        raise ValueError("candidate patch directory is missing or symlinked: ui/patches")
    for directory, directory_names, file_names in os.walk(patch_root, followlinks=False):
        directory_path = Path(directory)
        for name in directory_names:
            relative = (directory_path / name).relative_to(source).as_posix()
            if (directory_path / name).is_symlink():
                raise ValueError(f"candidate patch path is symlinked: {relative}")
        for name in file_names:
            path = directory_path / name
            relative = path.relative_to(source).as_posix()
            if path.suffix != ".patch":
                raise ValueError(f"unknown candidate patch input: {relative}")
            _regular_file(source, relative)
            inputs.append(relative)
    return tuple(sorted(inputs))


def materialize(source: Path, destination: Path) -> tuple[str, ...]:
    source = source.resolve()
    destination = destination.resolve()
    if destination.exists():
        raise ValueError("candidate dependency destination already exists")
    inputs = candidate_dependency_sources(source)
    for relative in inputs:
        source_path = source / relative
        destination_path = destination / relative
        destination_path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source_path, destination_path)
    return inputs


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    materialize(args.source, args.destination)
    print(args.destination.resolve())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
