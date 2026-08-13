#!/usr/bin/env python3
"""Validate and execute every claim transaction test exactly once."""

from __future__ import annotations

import json
import re
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Sequence

from claim_manifest import parse_claim_manifest

ROOT = Path(__file__).resolve().parents[1]
CLAIMS = ROOT / "verification" / "claims.tsv"
MANIFEST = ROOT / "verification" / "claim-test-manifest.tsv"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


@dataclass(frozen=True)
class ClaimTest:
    runner: str
    target: str
    symbol: str
    selector: str


CommandRunner = Callable[..., subprocess.CompletedProcess[str]]


def claim_test_links(claims_text: str) -> set[tuple[str, str]]:
    links: set[tuple[str, str]] = set()
    manifest = parse_claim_manifest(claims_text)
    for fields in manifest.rows:
        for link in fields[8].split(";"):
            if link.count("#") != 1:
                raise ValueError(f"invalid claim transaction test: {link}")
            links.add(tuple(link.split("#", 1)))
    return links


def runner_accepts(test: ClaimTest) -> bool:
    return (
        (
            test.runner == "rust-core"
            and test.target.startswith("canister/bridge-core/tests/")
            and test.target.endswith(".rs")
        )
        or (
            test.runner in {"rust-canister", "rust-canister-test-deployment"}
            and test.target.startswith("canister/bridge-canister/src/")
            and test.target.endswith(".rs")
        )
        or (
            test.runner == "foundry"
            and test.target.startswith("contracts/test/")
            and test.target.endswith(".t.sol")
        )
        or (
            test.runner == "vitest"
            and test.target.startswith("ui/src/")
            and test.target.endswith((".test.ts", ".test.tsx"))
        )
        or (test.runner == "jest" and test.target == "integration/phase3.spec.ts")
    )


def selector_binds_symbol(test: ClaimTest, source: str) -> bool:
    if test.selector == test.symbol:
        return True
    if test.runner not in {"vitest", "jest"}:
        return False
    registration = re.compile(
        rf"""\b(?:it|test)\s*\(\s*
        (?P<quote>["']){re.escape(test.selector)}(?P=quote)\s*,\s*
        {re.escape(test.symbol)}\s*[,)]
        """,
        re.VERBOSE,
    )
    return registration.search(source) is not None


def parse_manifest(
    claims_text: str,
    manifest_text: str,
    root: Path = ROOT,
) -> list[ClaimTest]:
    tests: list[ClaimTest] = []
    identities: set[tuple[str, str]] = set()
    for number, line in enumerate(manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 4 or not all(fields):
            raise ValueError(f"invalid claim test row {number}")
        test = ClaimTest(*fields)
        if not IDENTIFIER.fullmatch(test.symbol) or not runner_accepts(test):
            raise ValueError(
                f"unsupported claim test runner target: {test.runner} {test.target}"
            )
        path = (root / test.target).resolve()
        if root.resolve() not in path.parents or not path.is_file():
            raise ValueError(f"claim test target is missing: {test.target}")
        source = path.read_text(encoding="utf-8")
        if re.search(
            rf"\b{re.escape(test.symbol)}\b", source
        ) is None:
            raise ValueError(f"claim test symbol is missing: {test.symbol}")
        if not selector_binds_symbol(test, source):
            raise ValueError(
                f"claim test selector is not bound to symbol: "
                f"{test.selector} -> {test.symbol}"
            )
        identity = (test.target, test.symbol)
        if identity in identities:
            raise ValueError(f"duplicate claim test: {identity}")
        identities.add(identity)
        tests.append(test)
    expected = claim_test_links(claims_text)
    if identities != expected:
        raise ValueError(
            f"claim test manifest {sorted(identities)} does not match claims "
            f"{sorted(expected)}"
        )
    return tests


def run_command(
    command: Sequence[str],
    root: Path,
    runner: CommandRunner = subprocess.run,
) -> subprocess.CompletedProcess[str]:
    result = runner(command, cwd=root, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise ValueError(
            f"claim transaction test failed: {' '.join(command)}\n"
            f"{result.stdout}{result.stderr}"
        )
    return result


def prepare_test_dependencies(
    tests: Sequence[ClaimTest],
    root: Path = ROOT,
    runner: CommandRunner = subprocess.run,
) -> None:
    if not any(test.runner == "jest" for test in tests):
        return
    run_command(
        [
            "cargo",
            "build",
            "--locked",
            "--manifest-path",
            str(root / "Cargo.toml"),
            "--target-dir",
            str(root / "target/test-deployment"),
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "-p",
            "bridge-canister",
            "--features",
            "test-deployment",
        ],
        root,
        runner,
    )
    run_command(
        [
            "cargo",
            "build",
            "--locked",
            "--manifest-path",
            str(root / "Cargo.toml"),
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "-p",
            "mock-external",
        ],
        root,
        runner,
    )


def execute_test(
    test: ClaimTest,
    root: Path = ROOT,
    runner: CommandRunner = subprocess.run,
) -> None:
    if test.runner == "rust-core":
        result = run_command(
            [
                "cargo",
                "test",
                "--locked",
                "-p",
                "bridge-core",
                "--test",
                Path(test.target).stem,
                test.selector,
                "--",
                "--exact",
            ],
            root,
            runner,
        )
        output = result.stdout + result.stderr
        expected = rf"^test {re.escape(test.selector)} \.\.\. ok$"
        if len(re.findall(r"^running 1 test$", output, re.MULTILINE)) != 1 or len(
            re.findall(expected, output, re.MULTILINE)
        ) != 1:
            raise ValueError(f"Rust claim test did not pass exactly once: {test.selector}")
    elif test.runner in {"rust-canister", "rust-canister-test-deployment"}:
        command = ["cargo", "test", "--locked", "-p", "bridge-canister"]
        if test.runner == "rust-canister-test-deployment":
            command.extend(["--features", "test-deployment"])
        command.append(test.selector)
        result = run_command(
            command,
            root,
            runner,
        )
        output = result.stdout + result.stderr
        expected = rf"^test .*::{re.escape(test.selector)} \.\.\. ok$"
        if len(re.findall(expected, output, re.MULTILINE)) != 1:
            raise ValueError(f"Rust claim test did not pass exactly once: {test.selector}")
    elif test.runner == "foundry":
        result = run_command(
            [
                "forge",
                "test",
                "--root",
                "contracts",
                "--match-path",
                test.target.removeprefix("contracts/"),
                "--match-test",
                test.selector,
                "--json",
            ],
            root,
            runner,
        )
        report = json.loads(result.stdout)
        results = [
            (name, value)
            for suite in report.values()
            for name, value in suite.get("test_results", {}).items()
        ]
        if (
            len(results) != 1
            or results[0][0] != f"{test.selector}()"
            or results[0][1].get("status") != "Success"
        ):
            raise ValueError(
                f"Foundry claim test did not pass exactly once: {test.selector}"
            )
    elif test.runner == "vitest":
        result = run_command(
            [
                "pnpm",
                "--dir",
                "ui",
                "exec",
                "vitest",
                "run",
                test.target.removeprefix("ui/"),
                "-t",
                test.selector,
                "--reporter=json",
            ],
            root,
            runner,
        )
        report = json.loads(result.stdout)
        matches = [
            assertion
            for item in report.get("testResults", [])
            for assertion in item.get("assertionResults", [])
            if assertion.get("title") == test.selector
            and assertion.get("status") == "passed"
        ]
        if report.get("numPassedTests") != 1 or len(matches) != 1:
            raise ValueError(f"Vitest claim test did not pass exactly once: {test.selector}")
    elif test.runner == "jest":
        with tempfile.TemporaryDirectory() as directory:
            report_path = Path(directory) / "jest-report.json"
            run_command(
                [
                    "pnpm",
                    "exec",
                    "jest",
                    "--config",
                    "integration/jest.config.js",
                    "--runInBand",
                    test.target,
                    "-t",
                    test.selector,
                    "--json",
                    "--outputFile",
                    str(report_path),
                ],
                root,
                runner,
            )
            report = json.loads(report_path.read_text(encoding="utf-8"))
        matches = [
            assertion
            for item in report.get("testResults", [])
            for assertion in item.get("assertionResults", [])
            if assertion.get("title") == test.selector
            and assertion.get("status") == "passed"
        ]
        if report.get("numPassedTests") != 1 or len(matches) != 1:
            raise ValueError(f"Jest claim test did not pass exactly once: {test.selector}")
    else:
        raise ValueError(f"unknown claim test runner: {test.runner}")


def main() -> int:
    tests = parse_manifest(
        CLAIMS.read_text(encoding="utf-8"),
        MANIFEST.read_text(encoding="utf-8"),
    )
    prepare_test_dependencies(tests)
    for test in tests:
        execute_test(test)
        print(f"claim transaction test passed: {test.runner} {test.symbol}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
