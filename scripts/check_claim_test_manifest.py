#!/usr/bin/env python3
"""Validate and execute every claim transaction test exactly once."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
import time
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Sequence

from claim_manifest import parse_claim_manifest, parse_claim_test_links

ROOT = Path(__file__).resolve().parents[1]
CLAIMS = ROOT / "verification" / "claims.tsv"
MANIFEST = ROOT / "verification" / "claim-test-manifest.tsv"
LINKS = ROOT / "verification" / "claim-test-links.tsv"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


@dataclass(frozen=True)
class ClaimTest:
    runner: str
    target: str
    symbol: str
    selector: str
    execution: str = "grouped"
    claims: tuple[str, ...] = ()


CommandRunner = Callable[..., subprocess.CompletedProcess[str]]


def unique_json_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate test report key: {key}")
        result[key] = value
    return result


def parse_json_report(output: str) -> object:
    """Decode a JSON reporter payload while tolerating package-manager warnings."""
    decoder = json.JSONDecoder(object_pairs_hook=unique_json_object)
    for match in re.finditer(r"(?m)^\s*(?=[{\[])", output):
        candidate = output[match.end() :]
        try:
            report, end = decoder.raw_decode(candidate)
        except json.JSONDecodeError:
            continue
        if not candidate[end:].strip():
            return report
    raise ValueError("test reporter did not emit one terminal JSON payload")


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
            test.runner == "rust-profile"
            and test.target == "tools/bridge-profile/src/main.rs"
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
    links_by_claim = parse_claim_test_links(
        claims_text, (root / "verification/claim-test-links.tsv").read_text(encoding="utf-8")
    )
    tests: list[ClaimTest] = []
    identities: set[tuple[str, str]] = set()
    selectors: set[tuple[str, str, str]] = set()
    for number, line in enumerate(manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 5 or not all(fields):
            raise ValueError(f"invalid claim test row {number}")
        test = ClaimTest(*fields, claims=tuple(sorted(
            claim for claim, links in links_by_claim.items()
            if fields[1] + "#" + fields[2] in links
        )))
        if test.execution != "grouped" and not re.fullmatch(
            r"isolated:[a-z][a-z0-9-]*", test.execution
        ):
            raise ValueError(f"invalid claim test execution policy: {test.execution}")
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
        selector_identity = (test.runner, test.target, test.selector)
        if selector_identity in selectors:
            raise ValueError(f"duplicate claim test selector: {selector_identity}")
        selectors.add(selector_identity)
        tests.append(test)
    expected = {tuple(link.split("#", 1)) for links in links_by_claim.values() for link in links}
    if identities != expected:
        raise ValueError(
            f"claim test manifest {sorted(identities)} does not match claims "
            f"{sorted(expected)}"
        )
    return tests


def group_tests(tests: Sequence[ClaimTest]) -> list[list[ClaimTest]]:
    groups: dict[tuple[str, str, str], list[ClaimTest]] = {}
    for test in tests:
        key = (test.runner, test.target, test.symbol if test.execution != "grouped" else "")
        groups.setdefault(key, []).append(test)
    return list(groups.values())


def select_claim_tests(
    claims_text: str, tests: Sequence[ClaimTest], selected_claims: object
) -> list[ClaimTest]:
    if (
        not isinstance(selected_claims, list)
        or not selected_claims
        or not all(isinstance(claim, str) for claim in selected_claims)
        or len(set(selected_claims)) != len(selected_claims)
    ):
        raise ValueError("selected claims must be a non-empty unique string list")
    claims = {row[1]: row for row in parse_claim_manifest(claims_text).rows}
    if set(selected_claims) - claims.keys():
        raise ValueError("selected claims contain an unknown claim")
    selected = [test for test in tests if set(test.claims).intersection(selected_claims)]
    if set(selected_claims) - {claim for test in selected for claim in test.claims}:
        raise ValueError("selected claim tests do not cover every required test")
    return selected


def check_passes(expected: Sequence[str], results: Sequence[tuple[str, str]]) -> None:
    if (
        len(set(expected)) != len(expected)
        or Counter(name for name, _ in results) != Counter(expected)
        or any(status != "passed" for _, status in results)
    ):
        raise ValueError(f"claim tests did not pass exactly once: expected={list(expected)} results={list(results)}")


def check_js_report(report: object, tests: Sequence[ClaimTest], root: Path) -> None:
    if not isinstance(report, dict) or report.get("success") is not True:
        raise ValueError("invalid or failing JavaScript test report")
    expected = [test.selector for test in tests]
    assertions: list[tuple[str, str]] = []
    full_names: set[str] = set()
    for suite in report.get("testResults", []):
        if not isinstance(suite, dict) or not isinstance(suite.get("name"), str):
            raise ValueError("JavaScript test report has no file identity")
        if Path(suite["name"]).resolve() != (root / tests[0].target).resolve():
            raise ValueError("JavaScript test report file does not match the execution plan")
        for assertion in suite.get("assertionResults", []):
            if not isinstance(assertion, dict):
                raise ValueError("invalid JavaScript assertion result")
            title, status = assertion.get("title"), assertion.get("status")
            if title in expected:
                ancestors, full_name = assertion.get("ancestorTitles"), assertion.get("fullName")
                if (
                    not isinstance(ancestors, list)
                    or not all(isinstance(name, str) for name in ancestors)
                    or full_name != " ".join([*ancestors, title])
                    or full_name in full_names
                ):
                    raise ValueError("ambiguous JavaScript test identity")
                full_names.add(full_name)
                assertions.append((title, status))
            elif status not in {"pending", "skipped", "todo", "disabled"}:
                raise ValueError(f"unexpected executed JavaScript test: {title}")
    check_passes(expected, assertions)
    if (
        type(report.get("numPassedTests")) is not int
        or type(report.get("numFailedTests")) is not int
        or report["numPassedTests"] != len(expected)
        or report["numFailedTests"] != 0
    ):
        raise ValueError("JavaScript test report totals do not match the execution plan")


def execute_group(
    tests: Sequence[ClaimTest], root: Path = ROOT, runner: CommandRunner = subprocess.run
) -> None:
    if not tests or len(group_tests(tests)) != 1:
        raise ValueError("execution requires one non-empty runner/file/policy group")
    test = tests[0]
    expected = [item.selector for item in tests]
    if len(set(expected)) != len(expected):
        raise ValueError("ambiguous claim test selectors")
    # Match literal titles, including an optional enclosing JS suite prefix.
    pattern = "(?:^| )(?:" + "|".join(re.escape(name) for name in expected) + ")$"
    if test.runner in {"vitest", "jest"}:
        if test.runner == "vitest":
            result = run_command(
                ["pnpm", "--dir", "ui", "exec", "vitest", "run",
                 test.target.removeprefix("ui/"), "-t", pattern, "--reporter=json"], root, runner
            )
            report = parse_json_report(result.stdout)
        else:
            with tempfile.TemporaryDirectory() as directory:
                report_path = Path(directory) / "jest-report.json"
                run_command(
                    ["pnpm", "exec", "jest", "--config", "integration/jest.config.js",
                     "--runInBand", "--runTestsByPath", test.target, "-t", pattern,
                     "--json", "--outputFile", str(report_path)], root, runner
                )
                report = json.loads(report_path.read_text(encoding="utf-8"), object_pairs_hook=unique_json_object)
        check_js_report(report, tests, root)
    elif test.runner == "foundry":
        result = run_command(
            ["forge", "test", "--root", "contracts", "--match-path",
             test.target.removeprefix("contracts/"), "--match-test",
             "^(?:" + "|".join(re.escape(name) for name in expected) + r")\(\)$", "--json"],
            root, runner
        )
        report = parse_json_report(result.stdout)
        if not isinstance(report, dict):
            raise ValueError("invalid Foundry report")
        results = []
        for suite_name, suite in report.items():
            if suite_name.split(":", 1)[0] != test.target.removeprefix("contracts/"):
                raise ValueError("Foundry report file does not match the execution plan")
            if not isinstance(suite, dict) or not isinstance(suite.get("test_results"), dict):
                raise ValueError("invalid Foundry suite results")
            for name, value in suite["test_results"].items():
                if not isinstance(value, dict):
                    raise ValueError("invalid Foundry assertion")
                results.append((name, "passed" if value.get("status") == "Success" else "failed"))
        check_passes([name + "()" for name in expected], results)
    elif test.runner in {
        "rust-core",
        "rust-canister",
        "rust-canister-test-deployment",
        "rust-profile",
    }:
        command = ["cargo", "test", "--locked", "-p"]
        if test.runner == "rust-core":
            command += ["bridge-core", "--test", Path(test.target).stem]
        elif test.runner == "rust-profile":
            command += ["bridge-profile"]
        else:
            command += ["bridge-canister", "--lib"]
            if test.runner == "rust-canister-test-deployment":
                command += ["--features", "test-deployment"]
        listed = run_command(command + ["--", "--list", "--format=terse"], root, runner)
        names = re.findall(r"^([^\r\n]+): test$", listed.stdout, re.MULTILINE)
        identities = []
        for selector in expected:
            if test.runner == "rust-core":
                matches = [name for name in names if name == selector]
            elif test.runner == "rust-profile":
                matches = [name for name in names if name.endswith("::" + selector)]
            else:
                module_path = Path(test.target).relative_to("canister/bridge-canister/src")
                parts = list(module_path.with_suffix("").parts)
                if parts[-1] in {"mod", "lib"}:
                    parts.pop()
                prefix = "::".join(parts)
                matches = [name for name in names if name.endswith("::" + selector)
                           and (not prefix or name.startswith(prefix + "::"))]
            if len(matches) != 1:
                raise ValueError(f"Rust selector did not resolve exactly once: {selector}")
            identities.append(matches[0])
        result = run_command(command + ["--", "--exact", "--test-threads=1", "--format=pretty", *identities], root, runner)
        results = re.findall(r"^test ([^\r\n]+) \.\.\. ([^\r\n]+)$", result.stdout, re.MULTILINE)
        check_passes(identities, [(name, "passed" if status == "ok" else status) for name, status in results])
        if re.findall(r"^running (\d+) tests?$", result.stdout, re.MULTILINE) != [str(len(tests))]:
            raise ValueError("Rust test totals do not match the execution plan")
    else:
        raise ValueError(f"unknown claim test runner: {test.runner}")


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
        [str(root / "scripts/plan007/build-staging-canister-wasm.sh")],
        root,
        runner,
    )
    run_command(
        [str(root / "scripts/plan007/build-schema35-predecessor-wasm.sh")],
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




def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--validate-only",
        action="store_true",
        help="validate claim/test registration without building or executing tests",
    )
    parser.add_argument("--plan-only", action="store_true", help="print the execution plan without building or executing")
    parser.add_argument("--impact-json", type=Path, help="run only claims selected by proofs-impacted; never produces a full proof receipt")
    args = parser.parse_args(argv)
    claims_text = CLAIMS.read_text(encoding="utf-8")
    tests = parse_manifest(
        claims_text,
        MANIFEST.read_text(encoding="utf-8"),
    )
    selected_claims = sorted(row[1] for row in parse_claim_manifest(claims_text).rows)
    if args.impact_json is not None:
        impact = json.loads(
            args.impact_json.read_text(encoding="utf-8"),
            object_pairs_hook=unique_json_object,
        )
        if not isinstance(impact, dict) or impact.get("unregistered") != []:
            raise ValueError("invalid or unregistered proof impact plan")
        stages = impact.get("stages")
        if (
            not isinstance(stages, list)
            or not all(isinstance(stage, str) for stage in stages)
            or len(set(stages)) != len(stages)
            or "claim-transaction-tests" not in stages
        ):
            raise ValueError("proof impact plan did not select claim transaction tests")
        selected_claims = impact.get("claims")
        tests = select_claim_tests(claims_text, tests, selected_claims)
    if args.validate_only:
        print(f"claim test manifest passed ({len(tests)} tests)")
        return 0
    groups = group_tests(tests)
    print(json.dumps({
        "claims": selected_claims,
        "tests": len(tests), "groups": len(groups),
        "runner_invocations": sum(2 if group[0].runner.startswith("rust-") else 1 for group in groups),
        "native_builds": sorted({test.runner for test in tests if test.runner.startswith("rust-") or test.runner == "foundry"}),
        "dependency_builds": ["staging-canister", "schema35-predecessor", "mock-external"]
            if any(test.runner == "jest" for test in tests) else [],
        "execution_plan": [{"runner": group[0].runner, "target": group[0].target,
                            "execution": group[0].execution,
                            "tests": [{"symbol": test.symbol, "selector": test.selector} for test in group]} for group in groups],
    }, indent=2), flush=True)
    if args.plan_only:
        return 0
    prepare_test_dependencies(tests)
    for index, group in enumerate(groups, 1):
        label = f"{index}/{len(groups)} {group[0].runner} {group[0].target} ({len(group)} tests)"
        print(f"claim transaction group started: {label}", flush=True)
        started = time.monotonic()
        try:
            execute_group(group)
        except Exception:
            print(f"claim transaction group failed: {label} elapsed_seconds={time.monotonic() - started:.3f}", flush=True)
            raise
        print(f"claim transaction group passed: {label} elapsed_seconds={time.monotonic() - started:.3f}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
