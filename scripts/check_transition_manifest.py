#!/usr/bin/env python3
"""Fail closed unless every production transition has typed, executable evidence."""

from __future__ import annotations

import re
import subprocess
import tempfile
from pathlib import Path

from claim_manifest import LEAN_NAME, parse_claim_manifest

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "verification" / "transition-manifest.tsv"
VERUS_MANIFEST = ROOT / "verification" / "verus" / "manifest.tsv"
VERUS_PASS = ROOT / "verification" / "verus" / "pass.rs"
KERNEL = ROOT / "canister" / "bridge-core" / "src" / "kernel.rs"
TRANSITION = re.compile(r"^\s*pub\s+(?:const\s+)?fn\s+(\w*transition\w*)\s*\(", re.MULTILINE)


def require_exact_coverage(watched: set[str], registered: set[str]) -> None:
    if watched != registered:
        raise ValueError(
            "transition coverage differs: "
            f"missing={sorted(watched - registered)} extra={sorted(registered - watched)}"
        )


def strip_comments_and_strings(source: str) -> str:
    """Preserve offsets while removing Rust comments and string/character contents."""
    result = list(source)
    index = 0
    state = "code"
    block_depth = 0
    while index < len(source):
        pair = source[index : index + 2]
        char = source[index]
        if state == "code":
            if pair == "//":
                result[index] = result[index + 1] = " "
                index += 2
                state = "line"
                continue
            if pair == "/*":
                result[index] = result[index + 1] = " "
                index += 2
                state = "block"
                block_depth = 1
                continue
            if char == '"':
                result[index] = " "
                state = "string"
            elif char == "'" and index + 2 < len(source):
                result[index] = " "
                state = "char"
        elif state == "line":
            if char == "\n":
                state = "code"
            else:
                result[index] = " "
        elif state == "block":
            result[index] = " "
            if pair == "/*":
                result[index + 1] = " "
                block_depth += 1
                index += 2
                continue
            if pair == "*/":
                result[index + 1] = " "
                block_depth -= 1
                index += 2
                if block_depth == 0:
                    state = "code"
                continue
        elif state in {"string", "char"}:
            result[index] = " "
            if char == "\\" and index + 1 < len(source):
                result[index + 1] = " "
                index += 2
                continue
            if (state == "string" and char == '"') or (state == "char" and char == "'"):
                state = "code"
        index += 1
    return "".join(result)


def function_body(source: str, name: str) -> str:
    cleaned = strip_comments_and_strings(source)
    marker = re.compile(rf"\b(?:pub\s+)?(?:const\s+)?fn\s+{re.escape(name)}\s*\(")
    match = marker.search(cleaned)
    if match is None:
        raise ValueError(f"missing function body: {name}")
    next_function = re.compile(r"\b(?:pub\s+)?(?:const\s+)?fn\s+\w+\s*\(").search(
        cleaned, match.end()
    )
    limit = next_function.start() if next_function else len(cleaned)
    depth = 0
    start = None
    candidates: list[tuple[int, int]] = []
    for index in range(match.end(), limit):
        if cleaned[index] == "{":
            if depth == 0:
                start = index
            depth += 1
        elif cleaned[index] == "}":
            if depth == 0:
                break
            depth -= 1
            if depth == 0:
                assert start is not None
                candidates.append((start, index + 1))
    if candidates:
        start, end = candidates[-1]
        return cleaned[start:end]
    semicolon = cleaned.find(";", match.end(), limit)
    if semicolon >= 0:
        raise ValueError(f"function has no body: {name}")
    raise ValueError(f"unbalanced function body: {name}")


def function_declaration(source: str, name: str) -> str:
    cleaned = strip_comments_and_strings(source)
    marker = re.compile(rf"\b(?:pub\s+)?(?:const\s+)?fn\s+{re.escape(name)}\s*\(")
    match = marker.search(cleaned)
    if match is None:
        raise ValueError(f"missing function declaration: {name}")
    body = function_body(source, name)
    body_start = cleaned.find(body, match.end())
    return cleaned[match.start() : body_start + len(body)]


def body_calls(body: str, symbol: str) -> bool:
    return re.search(rf"\bkernel\s*::\s*{re.escape(symbol)}\s*\(", body) is not None


def checked_production_link(link: str) -> str:
    if link.count("#") != 1:
        raise ValueError(f"invalid production link: {link}")
    path_text, symbol = link.split("#")
    path = ROOT / path_text
    if path.resolve() != KERNEL.resolve() or not path.is_file():
        raise ValueError(f"transition outside watched kernel: {link}")
    if re.search(rf"\b{re.escape(symbol)}\b", path.read_text(encoding="utf-8")) is None:
        raise ValueError(f"missing production transition: {link}")
    return symbol


def verus_rows() -> dict[str, tuple[str, str, str]]:
    rows: dict[str, tuple[str, str, str]] = {}
    for number, line in enumerate(VERUS_MANIFEST.read_text(encoding="utf-8").splitlines(), 1):
        kind, kernel, proof, fixture, _ = line.split("\t")
        if kernel in rows:
            raise ValueError(f"duplicate Verus kernel row {number}: {kernel}")
        rows[kernel] = (kind, proof, fixture)
    return rows


def check_lean_contracts(examples: list[tuple[str, str]]) -> None:
    source = ["import BridgeSpec.ClaimContracts", ""]
    source.extend(f"example : {contract} := {witness}" for contract, witness in examples)
    with tempfile.NamedTemporaryFile(mode="w", suffix=".lean", encoding="utf-8") as check:
        check.write("\n".join(source) + "\n")
        check.flush()
        result = subprocess.run(
            ["lake", "env", "lean", check.name],
            cwd=ROOT / "verification" / "lean",
            capture_output=True,
            text=True,
            check=False,
        )
    if result.returncode != 0:
        raise ValueError(f"Lean transition contract check failed:\n{result.stdout}{result.stderr}")


def main() -> int:
    claims = parse_claim_manifest(
        (ROOT / "verification" / "claims.tsv").read_text(encoding="utf-8")
    )
    claim_ids = {row[1] for row in claims.rows}
    watched = set(TRANSITION.findall(KERNEL.read_text(encoding="utf-8")))
    registered: set[str] = set()
    examples: list[tuple[str, str]] = []
    verus = verus_rows()
    pass_source = VERUS_PASS.read_text(encoding="utf-8")

    lines = MANIFEST.read_text(encoding="utf-8").splitlines()
    if not lines or lines[0].split("\t") != ["schema", "2", "-", "-", "-", "-", "-"]:
        raise ValueError("transition manifest must start with schema 2")
    for number, line in enumerate(lines[1:], 2):
        fields = line.split("\t")
        if len(fields) != 7 or not all(fields):
            raise ValueError(f"invalid transition manifest row {number}")
        production_link, lean_contract, lean_witness, kind, proof, fixture_link, claim_text = fields
        symbol = checked_production_link(production_link)
        if symbol in registered:
            raise ValueError(f"duplicate transition registration: {symbol}")
        registered.add(symbol)
        if LEAN_NAME.fullmatch(lean_contract) is None or LEAN_NAME.fullmatch(lean_witness) is None:
            raise ValueError(f"invalid Lean transition contract row {number}")
        examples.append((lean_contract, lean_witness))

        fixture = ROOT / fixture_link
        if not fixture.is_file():
            raise ValueError(f"missing transition negative fixture: {fixture_link}")
        expected = verus.get(symbol)
        actual = (kind, proof, fixture.name)
        if expected != actual:
            raise ValueError(f"Verus transition registration mismatch for {symbol}: {expected} != {actual}")
        proof_body = function_body(pass_source, proof)
        fixture_source = fixture.read_text(encoding="utf-8")
        fixture_name = re.search(
            r"\bfn\s+(\w+)\s*\(", strip_comments_and_strings(fixture_source)
        ).group(1)
        fixture_body = function_body(fixture_source, fixture_name)
        called_symbol = symbol if kind == "executable" else f"{symbol}_spec"
        proof_scope = proof_body if kind == "executable" else function_declaration(pass_source, proof)
        fixture_scope = fixture_body if kind == "executable" else function_declaration(
            fixture_source, fixture_name
        )
        if not body_calls(proof_scope, called_symbol):
            raise ValueError(
                f"Verus proof body does not call registered kernel: {proof} -> {called_symbol}"
            )
        if not body_calls(fixture_scope, called_symbol):
            raise ValueError(
                f"negative fixture body does not call registered kernel: {fixture_link}"
            )
        unknown = set(claim_text.split(";")) - claim_ids
        if unknown:
            raise ValueError(f"unknown transition claims: {sorted(unknown)}")

    require_exact_coverage(watched, registered)
    check_lean_contracts(examples)
    print(f"transition manifest passed ({len(registered)} transitions)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
