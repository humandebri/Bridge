#!/usr/bin/env python3
"""Create and verify canonical ABI snapshots without third-party dependencies."""

from __future__ import annotations

import argparse
import difflib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CONTRACTS = ROOT / "contracts"
SNAPSHOTS = CONTRACTS / "abi"
TARGETS = {
    "Bridge": "src/Bridge.sol:Bridge",
    "BSNS": "src/BSNS.sol:BSNS",
}
INTERFACES = {
    "Bridge": "src/interfaces/IBridge.sol:IBridge",
    "BSNS": "src/interfaces/IBSNS.sol:IBSNS",
}


def run_abi(identifier: str) -> list[dict[str, Any]]:
    command = [
        "forge",
        "inspect",
        "--root",
        str(CONTRACTS),
        identifier,
        "abi",
        "--json",
    ]
    result = subprocess.run(command, check=False, capture_output=True, text=True)
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return json.loads(result.stdout)


def strip_internal_types(value: Any) -> Any:
    if isinstance(value, dict):
        return {key: strip_internal_types(item) for key, item in value.items() if key != "internalType"}
    if isinstance(value, list):
        return [strip_internal_types(item) for item in value]
    return value


def strip_parameter_names(value: Any, *, top_level: bool = True) -> Any:
    if isinstance(value, dict):
        return {
            key: strip_parameter_names(item, top_level=False)
            for key, item in value.items()
            if top_level or key != "name"
        }
    if isinstance(value, list):
        return [strip_parameter_names(item, top_level=False) for item in value]
    return value


def canonical_abi(abi: list[dict[str, Any]], *, remove_internal_types: bool = False) -> list[dict[str, Any]]:
    normalized = [strip_internal_types(item) if remove_internal_types else item for item in abi]
    return sorted(normalized, key=lambda item: json.dumps(item, sort_keys=True, separators=(",", ":")))


def encoded(abi: list[dict[str, Any]]) -> str:
    return json.dumps(abi, indent=2, sort_keys=True) + "\n"


def check_interface_subset(name: str, concrete: list[dict[str, Any]], interface: list[dict[str, Any]]) -> None:
    concrete_keys = {
        json.dumps(strip_parameter_names(item), sort_keys=True, separators=(",", ":"))
        for item in canonical_abi(concrete, remove_internal_types=True)
    }
    missing = [
        item
        for item in canonical_abi(interface, remove_internal_types=True)
        if json.dumps(strip_parameter_names(item), sort_keys=True, separators=(",", ":")) not in concrete_keys
    ]
    if missing:
        sys.stderr.write(f"{name} concrete ABI is missing interface entries:\n")
        sys.stderr.write(encoded(missing))
        raise SystemExit(1)


def update() -> None:
    SNAPSHOTS.mkdir(parents=True, exist_ok=True)
    for name, identifier in TARGETS.items():
        path = SNAPSHOTS / f"{name}.json"
        path.write_text(encoded(canonical_abi(run_abi(identifier))), encoding="utf-8")


def check() -> None:
    for name, identifier in TARGETS.items():
        concrete = run_abi(identifier)
        interface = run_abi(INTERFACES[name])
        check_interface_subset(name, concrete, interface)
        expected = SNAPSHOTS / f"{name}.json"
        actual = encoded(canonical_abi(concrete))
        if not expected.exists():
            sys.stderr.write(f"missing ABI snapshot: {expected}\n")
            raise SystemExit(1)
        current = expected.read_text(encoding="utf-8")
        if current != actual:
            diff = difflib.unified_diff(
                current.splitlines(),
                actual.splitlines(),
                fromfile=str(expected),
                tofile=f"generated:{expected.name}",
                lineterm="",
            )
            sys.stderr.write("\n".join(diff) + "\n")
            raise SystemExit(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    args = parser.parse_args()
    if args.update:
        update()
    else:
        check()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
