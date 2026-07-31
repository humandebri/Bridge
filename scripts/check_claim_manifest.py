#!/usr/bin/env python3
"""Validate the unified typed claim manifest and compute evidence strength."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "verification" / "claims.tsv"
REPORT = ROOT / "verification" / "output" / "claim-report.json"
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
REQUIRED_SCALAR_CALLS = (
    "deadlineAccepts(",
    "epochMatches(",
    "depositAvailable(",
    "feeWithinBounds(",
    "mintAmountWithinLimit(",
    "windowExpired(",
    "MintAccounting.tryConsumeWindow(",
    "mintEffectAmounts(",
)


def items(value: str) -> list[str]:
    return [] if value == "-" else value.split(";")


def checked_link(value: str) -> tuple[Path, str]:
    if value.count("#") != 1:
        raise ValueError(f"invalid source link: {value}")
    path_text, symbol = value.split("#")
    if not IDENTIFIER.fullmatch(symbol):
        raise ValueError(f"invalid source symbol: {value}")
    path = (ROOT / path_text).resolve()
    if ROOT.resolve() not in path.parents or not path.is_file():
        raise ValueError(f"missing source link target: {value}")
    if re.search(rf"\b{re.escape(symbol)}\b", path.read_text(encoding="utf-8")) is None:
        raise ValueError(f"missing registered source symbol: {value}")
    return path, symbol


def solidity_function_body(source: str, name: str) -> str:
    marker = f"function {name}("
    start = source.find(marker)
    if start < 0:
        raise ValueError(f"missing Solidity function: {name}")
    brace = source.find("{", start)
    semicolon = source.find(";", start)
    if brace < 0 or (semicolon >= 0 and semicolon < brace):
        raise ValueError(f"Solidity function has no body: {name}")
    depth = 0
    for index in range(brace, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[brace : index + 1]
    raise ValueError(f"Solidity function body is not balanced: {name}")


def missing_scalar_calls(function_body: str) -> list[str]:
    return [call for call in REQUIRED_SCALAR_CALLS if call not in function_body]


def check_solidity_wrapper_refinement() -> None:
    policy = (
        ROOT / "contracts" / "src" / "libraries" / "MintAuthorizationPolicy.sol"
    ).read_text(encoding="utf-8")
    bridge = (ROOT / "contracts" / "src" / "Bridge.sol").read_text(encoding="utf-8")
    evaluate = solidity_function_body(policy, "evaluateMint")
    missing = missing_scalar_calls(evaluate)
    if missing:
        raise ValueError(f"Mint struct wrapper bypasses scalar kernels: {missing}")
    mint_wrapper = bridge[
        bridge.index("function mintDepositWithAuthorization") : bridge.index(
            "function createWithdrawal"
        )
    ]
    required_effect_applications = (
        "MintAuthorizationPolicy.evaluateMint(",
        "= effects.processedAfter;",
        "= effects.windowStartedAtAfter;",
        "= effects.windowConsumedAfter;",
        "effects.supplyIncrease",
        "effects.eventGrossAmount",
        "effects.eventServiceFee",
        "effects.eventMintedAmount",
    )
    missing = [value for value in required_effect_applications if value not in mint_wrapper]
    if missing or mint_wrapper.count("MintAuthorizationPolicy.evaluateMint(") != 1:
        raise ValueError(f"Bridge mint wrapper does not apply one exact transition: {missing}")


def main() -> int:
    check_solidity_wrapper_refinement()
    lean_source = "\n".join(
        path.read_text(encoding="utf-8")
        for path in sorted((ROOT / "verification" / "lean" / "BridgeSpec").glob("*.lean"))
    )
    lean_theorems = set(
        re.findall(r"^theorem ([A-Za-z_][A-Za-z0-9_]*)\b", lean_source, re.MULTILINE)
    )
    verus_source = (ROOT / "verification" / "verus" / "pass.rs").read_text(encoding="utf-8")
    verus_rows: dict[str, list[tuple[str, str]]] = {}
    for line in (ROOT / "verification" / "verus" / "manifest.tsv").read_text(
        encoding="utf-8"
    ).splitlines():
        kind, kernel, proof, _, _ = line.split("\t")
        registration = (kind, kernel)
        if registration in verus_rows.setdefault(proof, []):
            raise ValueError(f"duplicate Verus proof registration: {proof}/{kernel}")
        verus_rows[proof].append(registration)
    assumptions = {
        line.split("\t", 1)[0]
        for line in (ROOT / "verification" / "assumptions.tsv")
        .read_text(encoding="utf-8")
        .splitlines()
        if line
    }
    vector_sections = {
        line.split("\t", 1)[0]
        for line in (ROOT / "verification" / "refinement-manifest.tsv")
        .read_text(encoding="utf-8")
        .splitlines()
        if line
    }

    rows = [
        line.split("\t")
        for line in MANIFEST.read_text(encoding="utf-8").splitlines()
        if line
    ]
    if any(len(row) != 11 or not all(row) for row in rows):
        raise ValueError("unified claim manifest rows must have 11 non-empty columns")
    claim_ids = [row[1] for row in rows]
    if len(claim_ids) != len(set(claim_ids)):
        raise ValueError("duplicate unified claim id")

    used_verus: set[str] = set()
    results: list[dict[str, object]] = []
    for (
        kind,
        claim_id,
        abstract_theorems,
        refinement_theorems,
        trace_theorems,
        verus_obligations,
        smt_obligations,
        production_links,
        transaction_tests,
        assumption_ids,
        vectors,
    ) in rows:
        if kind not in {"protocol", "mint"} or not IDENTIFIER.fullmatch(claim_id):
            raise ValueError(f"invalid typed claim: {kind}/{claim_id}")
        theorem_names = (
            items(abstract_theorems)
            + items(refinement_theorems)
            + items(trace_theorems)
        )
        missing_theorems = set(theorem_names) - lean_theorems
        if missing_theorems:
            raise ValueError(
                f"missing Lean theorem for {claim_id}: {sorted(missing_theorems)}"
            )
        obligations = items(verus_obligations)
        unknown_verus = set(obligations) - set(verus_rows)
        if unknown_verus:
            raise ValueError(
                f"unknown Verus obligation for {claim_id}: {sorted(unknown_verus)}"
            )
        used_verus.update(obligations)
        smt_links = [checked_link(link) for link in items(smt_obligations)]
        production = [checked_link(link) for link in items(production_links)]
        tests = [checked_link(link) for link in items(transaction_tests)]
        unknown_assumptions = set(items(assumption_ids)) - assumptions
        if unknown_assumptions:
            raise ValueError(
                f"unknown assumption for {claim_id}: {sorted(unknown_assumptions)}"
            )
        if vectors != "-" and vectors not in vector_sections:
            raise ValueError(f"unknown refinement vector section for {claim_id}: {vectors}")

        production_text = "\n".join(
            path.read_text(encoding="utf-8") for path, _ in production
        )
        model_only = [
            obligation
            for obligation in obligations
            if all(kind == "model" for kind, _ in verus_rows[obligation])
        ]
        unreferenced_kernels = [
            kernel
            for obligation in obligations
            for registration_kind, kernel in verus_rows[obligation]
            if registration_kind != "model"
            and re.search(
                rf"\b{re.escape(kernel)}\b", production_text
            )
            is None
        ]
        kernel_strength = (
            "refinement-tested"
            if model_only or unreferenced_kernels
            else "implementation-proved"
        )
        evidence = {
            "abstract": "proved",
            "production_kernel": kernel_strength,
            "smt_scalar": "implementation-proved" if smt_links else "not-applicable",
            "adapter": "refinement-tested" if tests else "missing",
            "external": "assumed" if items(assumption_ids) else "not-applicable",
        }
        reasons: list[str] = []
        if model_only:
            reasons.append("model_only_verus:" + ",".join(sorted(model_only)))
        if unreferenced_kernels:
            reasons.append(
                "production_kernel_not_linked:" + ",".join(sorted(unreferenced_kernels))
            )
        if items(assumption_ids):
            reasons.append("external_assumptions:" + ",".join(items(assumption_ids)))
        status = "partial" if reasons else kernel_strength
        results.append(
            {
                "id": claim_id,
                "kind": kind,
                "status": status,
                "evidence": evidence,
                "unproved_reasons": reasons,
            }
        )

    missing_verus = set(verus_rows) - used_verus
    if missing_verus:
        raise ValueError(
            f"unregistered Verus obligations in unified claims: {sorted(missing_verus)}"
        )
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(
        json.dumps({"schema": 1, "claims": results}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"unified claim manifest passed ({len(results)} claims)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
