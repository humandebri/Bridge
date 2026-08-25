#!/usr/bin/env python3
"""Validate advisory Certora specs, configs, and claim ownership."""

from __future__ import annotations

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from check_solidity_ast_bindings import AstIndex


ROOT = Path(__file__).resolve().parents[1]
CERTORA = Path("verification/certora")
EXPECTED_CONFIGS = {
    "Bridge.conf": ("Bridge", "Bridge.spec"),
    "BSNS.conf": ("BSNS", "BSNS.spec"),
    "BridgeTimelockController.conf": (
        "BridgeTimelockController",
        "BridgeTimelockController.spec",
    ),
}
EXPECTED_CLI = "certora-cli==8.17.1"
EXPECTED_PROVER = "release/15June2026"
EXPECTED_YUL_STEPS = (
    "dfDvulfnTUtnIfxa[r]EscLMVcul[j]Trpeulxa[r]cLvifMCTUca[r]LSsTFOtfDnca[r]"
    "IulcscCTUtvifMx[scCTUt]TOntnfDIulvifMjmul[jul]VcTOculjmul"
)
EXPECTED_PACKAGES = {
    "Bridge.conf": {
        "@openzeppelin/contracts/=contracts/lib/openzeppelin-contracts/contracts",
        "bridge-src/=contracts/src",
        "bridge-deployment-policy/=contracts/src/policies/production",
    },
    "BSNS.conf": {
        "@openzeppelin/contracts/=contracts/lib/openzeppelin-contracts/contracts",
    },
    "BridgeTimelockController.conf": {
        "@openzeppelin/contracts/=contracts/lib/openzeppelin-contracts/contracts",
        "bridge-deployment-policy/=contracts/src/policies/production",
    },
}
FORBIDDEN_CONFIG_KEYS = {
    "assume_no_casting_overflow",
    "disable_local_type_checking",
    "optimistic_contract_recursion",
    "optimistic_fallback",
    "optimistic_hashing",
    "optimistic_loop",
}
RULE = re.compile(r"(?m)^\s*(?:rule|invariant)\s+([A-Za-z_][A-Za-z0-9_]*)\b")


@dataclass(frozen=True)
class Obligation:
    identifier: str
    rules: tuple[str, ...]
    sources: tuple[str, ...]
    assumptions: tuple[str, ...]
    claims: tuple[str, ...]


def split_entries(value: str) -> tuple[str, ...]:
    return tuple(value.split(";"))


def parse_link(value: str, root: Path, *, suffix: str) -> tuple[Path, str]:
    if value.count("#") != 1:
        raise ValueError(f"Certora link must contain one '#': {value}")
    raw_path, symbol = value.split("#", 1)
    path = root / raw_path
    if not path.is_file() or not symbol or not raw_path.endswith(suffix):
        raise ValueError(f"invalid Certora link: {value}")
    return path, symbol


def claim_ids(root: Path) -> set[str]:
    values: set[str] = set()
    for line in (root / "verification/claims.tsv").read_text(encoding="utf-8").splitlines()[1:]:
        fields = line.split("\t")
        if fields and fields[0] in {"contract", "protocol"}:
            if len(fields) < 2 or not fields[1]:
                raise ValueError("claims.tsv contains a missing claim id")
            # Each claim intentionally has a `contract` registration followed by
            # its `protocol` evidence row, so the same id appears in both kinds.
            values.add(fields[1])
    return values


def assumption_ids(root: Path) -> set[str]:
    return {
        line.split("\t", 1)[0]
        for line in (root / "verification/assumptions.tsv").read_text(encoding="utf-8").splitlines()
        if line
    }


def parse_obligations(root: Path) -> tuple[Obligation, ...]:
    path = root / CERTORA / "obligations.tsv"
    lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line]
    if not lines or lines[0].split("\t") != ["schema", "1", "-", "-", "-", "-", "-"]:
        raise ValueError("Certora obligations must use schema 1")
    obligations: list[Obligation] = []
    identifiers: set[str] = set()
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 7 or not all(fields):
            raise ValueError(f"invalid Certora obligation row {number}")
        kind, identifier, strength, rules, sources, assumptions, claims = fields
        if kind != "obligation" or strength != "advisory" or identifier in identifiers:
            raise ValueError(f"invalid Certora obligation identity at row {number}")
        identifiers.add(identifier)
        obligations.append(
            Obligation(
                identifier,
                split_entries(rules),
                split_entries(sources),
                split_entries(assumptions),
                split_entries(claims),
            )
        )
    if not obligations:
        raise ValueError("Certora obligations cannot be empty")
    return tuple(obligations)


def validate_configs(root: Path) -> None:
    config_dir = root / CERTORA / "confs"
    actual = {path.name for path in config_dir.glob("*.conf")}
    if actual != set(EXPECTED_CONFIGS):
        raise ValueError(f"unexpected Certora configs: {sorted(actual)}")
    for name, (contract, spec_name) in EXPECTED_CONFIGS.items():
        config = json.loads((config_dir / name).read_text(encoding="utf-8"))
        if set(config) & FORBIDDEN_CONFIG_KEYS:
            raise ValueError(f"{name} enables an under-approximating option")
        expected_verify = f"{contract}:verification/certora/specs/{spec_name}"
        required = {
            "verify": expected_verify,
            "solc": "certora-solc",
            "solc_evm_version": "prague",
            "solc_via_ir": True,
            "solc_optimize": "200",
            "yul_optimizer_steps": EXPECTED_YUL_STEPS,
            "prover_version": EXPECTED_PROVER,
            "rule_sanity": "advanced",
            "wait_for_results": "ALL",
            "url_visibility": "private",
            "process": "emv",
        }
        for key, value in required.items():
            if config.get(key) != value:
                raise ValueError(f"{name} must pin {key}={value!r}")
        if set(config.get("packages", [])) != EXPECTED_PACKAGES[name]:
            raise ValueError(f"{name} does not use the reviewed production remappings")
        files = config.get("files")
        if not isinstance(files, list) or not files or not all(
            isinstance(item, str) and (root / item).is_file() for item in files
        ):
            raise ValueError(f"{name} contains a missing Solidity input")


def validate_tool_pin(root: Path) -> None:
    pyproject = (root / CERTORA / "pyproject.toml").read_text(encoding="utf-8")
    if f'"{EXPECTED_CLI}"' not in pyproject:
        raise ValueError(f"Certora client must be pinned to {EXPECTED_CLI}")
    lock = root / CERTORA / "uv.lock"
    if not lock.is_file():
        raise ValueError("Certora uv.lock is missing")
    lock_text = lock.read_text(encoding="utf-8")
    if 'name = "certora-cli"' not in lock_text or 'version = "8.17.1"' not in lock_text:
        raise ValueError("Certora uv.lock does not pin certora-cli 8.17.1")


def validate(root: Path = ROOT, ast_index: AstIndex | None = None) -> None:
    validate_tool_pin(root)
    validate_configs(root)
    claims = claim_ids(root)
    assumptions = assumption_ids(root)
    obligations = parse_obligations(root)
    if ast_index is None:
        ast_index = AstIndex(
            root / "contracts" / "out", root / "contracts", root
        )

    declared: dict[str, Path] = {}
    for spec in sorted((root / CERTORA / "specs").glob("*.spec")):
        for rule in RULE.findall(spec.read_text(encoding="utf-8")):
            if rule in declared:
                raise ValueError(f"duplicate Certora rule: {rule}")
            declared[rule] = spec
    referenced: set[str] = set()
    for obligation in obligations:
        for link in obligation.rules:
            path, rule = parse_link(link, root, suffix=".spec")
            if declared.get(rule) != path:
                raise ValueError(f"Certora obligation references an undeclared rule: {link}")
            referenced.add(rule)
        unknown_assumptions = set(obligation.assumptions) - assumptions
        unknown_claims = set(obligation.claims) - claims
        if unknown_assumptions:
            raise ValueError(f"unknown Certora assumptions: {sorted(unknown_assumptions)}")
        if unknown_claims:
            raise ValueError(f"unknown Certora claims: {sorted(unknown_claims)}")
        for link in obligation.sources:
            source_path, _ = parse_link(link, root, suffix=".sol")
            records = ast_index.resolve(link)
            if len(records) != 1 or records[0].source != source_path.resolve():
                raise ValueError(
                    f"Certora source ownership must resolve exactly once: {link}"
                )
    unowned = set(declared) - referenced
    if unowned:
        raise ValueError(f"Certora rules missing obligation ownership: {sorted(unowned)}")


def main() -> int:
    try:
        validate()
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(error, file=sys.stderr)
        return 1
    print("Certora advisory manifest is valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
