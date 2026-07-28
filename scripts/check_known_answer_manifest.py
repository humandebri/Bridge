#!/usr/bin/env python3
"""Run every registered cryptographic known-answer consumer exactly once."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "verification" / "known-answer-manifest.tsv"
VECTOR = ROOT / "verification" / "generated" / "mint-authorization-vector.json"


def run(command: list[str]) -> str:
    result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise RuntimeError(f"known-answer consumer failed: {' '.join(command)}\n{result.stdout}{result.stderr}")
    return result.stdout + result.stderr


def main() -> int:
    vector = json.loads(VECTOR.read_text(encoding="utf-8"))
    if vector.get("schema_version") != 1:
        raise RuntimeError("Mint Authorization known-answer vector must be schema v1")
    rows = [line.split("\t") for line in MANIFEST.read_text(encoding="utf-8").splitlines() if line]
    if any(len(row) != 4 or not all(row) for row in rows):
        raise RuntimeError("invalid known-answer manifest row")
    identities = {(row[1], row[2], row[3]) for row in rows}
    if len(identities) != len(rows):
        raise RuntimeError("duplicate known-answer consumer")
    if {row[1] for row in rows} != {"rust", "foundry", "vitest"}:
        raise RuntimeError("Mint Authorization known-answer requires Rust, Foundry, and Vitest consumers")

    for kind, runner, target, selector in rows:
        if kind != "eip712_mint_authorization" or not (ROOT / target).is_file():
            raise RuntimeError(f"invalid known-answer target: {target}")
        if selector not in (ROOT / target).read_text(encoding="utf-8"):
            raise RuntimeError(f"known-answer selector is not present in target: {selector}")
        if runner == "rust":
            output = run(["cargo", "test", "--locked", "-p", "bridge-canister", selector])
            if len(re.findall(r"^test .*::" + re.escape(selector) + r" \.\.\. ok$", output, re.MULTILINE)) != 1:
                raise RuntimeError("Rust known-answer did not pass exactly once")
        elif runner == "foundry":
            report = json.loads(run([
                "forge", "test", "--root", "contracts", "--match-path", "test/ProtocolVectors.t.sol",
                "--match-test", selector, "--json",
            ]))
            results = [(name, result) for suite in report.values() for name, result in suite.get("test_results", {}).items()]
            if len(results) != 1 or results[0][0] != f"{selector}()" or results[0][1].get("status") != "Success":
                raise RuntimeError("Foundry known-answer did not pass exactly once")
        elif runner == "vitest":
            report = json.loads(run([
                "pnpm", "--dir", "ui", "exec", "vitest", "run", target.removeprefix("ui/"),
                "-t", selector, "--reporter=json",
            ]))
            matches = [
                assertion for test in report.get("testResults", [])
                for assertion in test.get("assertionResults", [])
                if assertion.get("title") == selector and assertion.get("status") == "passed"
            ]
            if report.get("numPassedTests") != 1 or len(matches) != 1:
                raise RuntimeError("Vitest known-answer did not pass exactly once")
        else:
            raise RuntimeError(f"unknown known-answer runner: {runner}")
        print(f"known-answer consumer passed: {runner} {selector}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
