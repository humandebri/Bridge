#!/usr/bin/env python3
"""Fingerprint the complete input boundary of advisory Certora evidence."""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
import sys
from pathlib import Path

from proof_fingerprint import validate_fingerprint


ROOT = Path(__file__).resolve().parents[1]
CERTORA_EXACT_INPUTS = (
    ".github/workflows/certora-advisory.yml",
    ".gitmodules",
    "contracts/foundry.toml",
    "contracts/test/BridgeTimelock.t.sol",
    "scripts/certora_fingerprint.py",
    "scripts/certora_results.py",
    "scripts/check_certora_manifest.py",
    "scripts/install-certora-solc.sh",
    "scripts/proof_fingerprint.py",
    "scripts/run_certora_advisory.sh",
    "scripts/test_certora_fingerprint.py",
    "scripts/test_certora_manifest.py",
    "scripts/test_certora_results.py",
    "verification/assumptions.tsv",
    "verification/claims.tsv",
)
CERTORA_SOURCE_ROOTS = (
    ("contracts/src", frozenset({".sol"})),
    ("contracts/lib/openzeppelin-contracts/contracts", frozenset({".sol"})),
    ("verification/certora", None),
)
CERTORA_EXCLUDED_PARTS = frozenset({".venv", "__pycache__"})
CERTORA_PYTHON_SEEDS = (
    "scripts/check_certora_manifest.py",
    "scripts/certora_results.py",
)


def _local_python_module(
    module: str,
    *,
    importer: Path,
    level: int,
    repo_root: Path,
) -> Path | None:
    if level:
        base = importer.parent
        for _ in range(level - 1):
            base = base.parent
    else:
        base = repo_root / "scripts"
    parts = tuple(part for part in module.split(".") if part)
    candidates = (base.joinpath(*parts).with_suffix(".py"), base.joinpath(*parts, "__init__.py"))
    existing = [path for path in candidates if path.is_file()]
    if len(existing) > 1:
        raise ValueError(f"ambiguous local Python import {module!r} from {importer}")
    return existing[0] if existing else None


def certora_python_dependency_paths(repo_root: Path = ROOT) -> frozenset[str]:
    pending = [repo_root / relative for relative in CERTORA_PYTHON_SEEDS]
    resolved: set[Path] = set()
    while pending:
        path = pending.pop()
        if path in resolved:
            continue
        if not path.is_file():
            raise ValueError(f"missing Certora Python dependency: {path.relative_to(repo_root)}")
        resolved.add(path)
        try:
            tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        except (OSError, SyntaxError) as error:
            raise ValueError(
                f"cannot parse Certora Python dependency: {path.relative_to(repo_root)}"
            ) from error
        imports: list[tuple[str, int]] = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                imports.extend((alias.name, 0) for alias in node.names)
            elif isinstance(node, ast.ImportFrom):
                imports.append((node.module or "", node.level))
        for module, level in imports:
            local = _local_python_module(
                module,
                importer=path,
                level=level,
                repo_root=repo_root,
            )
            if local is not None:
                pending.append(local)
                continue
            top_level = module.partition(".")[0]
            if level or (top_level and top_level not in sys.stdlib_module_names):
                relative = path.relative_to(repo_root).as_posix()
                raise ValueError(f"unresolved local Python import {module!r} from {relative}")
    return frozenset(path.relative_to(repo_root).as_posix() for path in resolved)


def certora_fingerprint_inputs(repo_root: Path = ROOT) -> tuple[Path, ...]:
    paths = {repo_root / relative for relative in CERTORA_EXACT_INPUTS}
    paths.update(repo_root / relative for relative in certora_python_dependency_paths(repo_root))
    for relative_root, suffixes in CERTORA_SOURCE_ROOTS:
        source_root = repo_root / relative_root
        if not source_root.is_dir():
            raise ValueError(f"missing Certora fingerprint root: {relative_root}")
        paths.update(
            path
            for path in source_root.rglob("*")
            if path.is_file()
            and not any(part in CERTORA_EXCLUDED_PARTS for part in path.parts)
            and (suffixes is None or path.suffix in suffixes)
        )
    missing = [path for path in paths if not path.is_file()]
    if missing:
        raise ValueError(
            "missing Certora fingerprint inputs: "
            + ", ".join(
                path.relative_to(repo_root).as_posix() for path in sorted(missing)
            )
        )
    return tuple(sorted(paths))


def certora_source_fingerprint(repo_root: Path = ROOT) -> dict[str, object]:
    digest = hashlib.sha256()
    inputs = certora_fingerprint_inputs(repo_root)
    for path in inputs:
        relative = path.relative_to(repo_root).as_posix().encode()
        content = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "big"))
        digest.update(relative)
        digest.update(len(content).to_bytes(8, "big"))
        digest.update(content)
    return {
        "algorithm": "sha256",
        "digest": digest.hexdigest(),
        "input_count": len(inputs),
        "scope": "certora-advisory-v1",
    }


def validate_certora_fingerprint(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or set(value) != {
        "algorithm",
        "digest",
        "input_count",
        "scope",
    }:
        raise ValueError("Certora source fingerprint has an invalid shape")
    core = {key: value[key] for key in ("algorithm", "digest", "input_count")}
    validate_fingerprint(core)
    if value["scope"] != "certora-advisory-v1":
        raise ValueError("Certora source fingerprint has an invalid scope")
    return value


def write_fingerprint(path: Path, repo_root: Path = ROOT) -> dict[str, object]:
    value = certora_source_fingerprint(repo_root)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")
    return value


def check_fingerprint(path: Path, repo_root: Path = ROOT) -> dict[str, object]:
    expected = json.loads(path.read_text(encoding="utf-8"))
    validate_certora_fingerprint(expected)
    actual = certora_source_fingerprint(repo_root)
    if expected != actual:
        raise ValueError("Certora inputs changed after the advisory run started")
    return actual


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", type=Path)
    mode.add_argument("--check", type=Path)
    args = parser.parse_args()
    try:
        if args.write is not None:
            write_fingerprint(args.write)
        else:
            check_fingerprint(args.check)
    except (OSError, ValueError, json.JSONDecodeError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
