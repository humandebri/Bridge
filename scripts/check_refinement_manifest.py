#!/usr/bin/env python3
"""Validate Lean refinement links and execute every registered consumer test."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Sequence


ROOT = Path(__file__).resolve().parents[1]
VECTORS = ROOT / "verification" / "generated" / "protocol-vectors.json"
MANIFEST = ROOT / "verification" / "refinement-manifest.tsv"
MODEL = ROOT / "verification" / "lean" / "BridgeSpec" / "Model.lean"
THEOREMS = ROOT / "verification" / "lean" / "BridgeSpec" / "Theorems.lean"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
RUNNER_TARGETS = {
    ("rust", "canister/bridge-core/tests/protocol_vectors.rs"),
    ("foundry", "contracts/test/ProtocolVectors.t.sol"),
    ("vitest", "ui/src/lib/protocol-vectors.test.ts"),
}


@dataclass(frozen=True)
class Consumer:
    section: str
    definition: str
    theorem: str
    runner: str
    target: str
    selector: str


CommandRunner = Callable[..., subprocess.CompletedProcess[str]]


def declaration(source: str, keyword: str, name: str) -> str:
    start = re.search(rf"^{keyword} {re.escape(name)}\b", source, re.MULTILINE)
    if start is None:
        raise ValueError(f"Lean {keyword} is missing: {name}")
    following = re.search(r"^(?:theorem|end)\b", source[start.end() :], re.MULTILINE)
    end = len(source) if following is None else start.end() + following.start()
    return source[start.start() : end]


def checked_target(root: Path, runner: str, target: str) -> Path:
    relative = Path(target)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"refinement consumer path must stay inside the repository: {target}")
    if (runner, target) not in RUNNER_TARGETS:
        raise ValueError(f"unsupported refinement runner target: {runner} {target}")
    path = (root / relative).resolve()
    if root.resolve() not in path.parents or not path.is_file():
        raise ValueError(f"refinement consumer is missing: {target}")
    return path


def parse_manifest(
    document: dict[str, object], manifest_text: str, model: str, theorems: str, root: Path
) -> list[Consumer]:
    if document.get("schema_version") != 2:
        raise ValueError("protocol vector schema must be exactly v2")
    vector_sections = {
        key
        for key, value in document.items()
        if key.endswith("_cases") and isinstance(value, list)
    }
    for section in vector_sections:
        if not document[section]:
            raise ValueError(f"protocol vector section is empty: {section}")

    consumers: list[Consumer] = []
    associations: dict[str, tuple[str, str]] = {}
    seen_consumers: set[tuple[str, str, str]] = set()
    for number, line in enumerate(manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 6 or not all(fields):
            raise ValueError(f"invalid refinement manifest row {number}")
        consumer = Consumer(*fields)
        if not all(IDENTIFIER.fullmatch(value) for value in (
            consumer.section,
            consumer.definition,
            consumer.theorem,
            consumer.selector,
        )):
            raise ValueError(f"invalid refinement identifier in row {number}")
        association = (consumer.definition, consumer.theorem)
        previous = associations.setdefault(consumer.section, association)
        if previous != association:
            raise ValueError(f"conflicting refinement association: {consumer.section}")
        identity = (consumer.runner, consumer.target, consumer.selector)
        if identity in seen_consumers:
            raise ValueError(f"duplicate refinement consumer: {' '.join(identity)}")
        seen_consumers.add(identity)
        checked_target(root, consumer.runner, consumer.target)
        consumers.append(consumer)

    if set(associations) != vector_sections:
        raise ValueError(
            f"refinement manifest sections {sorted(associations)} do not match "
            f"vectors {sorted(vector_sections)}"
        )
    for section, (definition, theorem) in associations.items():
        declaration(model, "def", definition)
        theorem_source = declaration(theorems, "theorem", theorem)
        theorem_statement = theorem_source.split(":= by", 1)[0]
        if re.search(rf"\b{re.escape(definition)}\b", theorem_statement) is None:
            raise ValueError(
                f"Lean theorem {theorem} does not directly reference {definition} for {section}"
            )
    return consumers


def run_command(command: Sequence[str], cwd: Path, runner: CommandRunner) -> subprocess.CompletedProcess[str]:
    result = runner(command, cwd=cwd, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise ValueError(
            f"refinement consumer command failed: {' '.join(command)}\n{result.stdout}{result.stderr}"
        )
    return result


def validate_rust(consumer: Consumer, output: str) -> None:
    selector = re.escape(consumer.selector)
    if len(re.findall(r"^running 1 test$", output, re.MULTILINE)) != 1:
        raise ValueError(f"Rust consumer did not execute exactly one test: {consumer.selector}")
    if len(re.findall(rf"^test {selector} \.\.\. ok$", output, re.MULTILINE)) != 1:
        raise ValueError(f"Rust consumer test did not pass: {consumer.selector}")
    if not re.search(r"^test result: ok\. 1 passed; 0 failed;", output, re.MULTILINE):
        raise ValueError(f"Rust consumer result count is invalid: {consumer.selector}")


def validate_foundry(consumer: Consumer, output: str) -> None:
    try:
        report = json.loads(output)
    except json.JSONDecodeError as error:
        raise ValueError(f"Foundry consumer returned invalid JSON: {consumer.selector}") from error
    results = [
        (name, result)
        for suite in report.values()
        for name, result in suite.get("test_results", {}).items()
    ]
    expected = f"{consumer.selector}()"
    if len(results) != 1 or results[0][0] != expected or results[0][1].get("status") != "Success":
        raise ValueError(f"Foundry consumer did not pass exactly once: {consumer.selector}")


def validate_vitest(consumer: Consumer, output: str) -> None:
    try:
        report = json.loads(output)
    except json.JSONDecodeError as error:
        raise ValueError(f"Vitest consumer returned invalid JSON: {consumer.selector}") from error
    assertions = [
        assertion
        for result in report.get("testResults", [])
        for assertion in result.get("assertionResults", [])
        if assertion.get("title") == consumer.selector
    ]
    if (
        report.get("success") is not True
        or report.get("numPassedTests") != 1
        or report.get("numFailedTests") != 0
        or len(assertions) != 1
        or assertions[0].get("status") != "passed"
    ):
        raise ValueError(f"Vitest consumer did not pass exactly once: {consumer.selector}")


def execute_consumer(consumer: Consumer, root: Path, runner: CommandRunner = subprocess.run) -> None:
    if consumer.runner == "rust":
        result = run_command(
            [
                "cargo", "test", "--locked", "-p", "bridge-core", "--test", "protocol_vectors",
                consumer.selector, "--", "--exact",
            ],
            root,
            runner,
        )
        validate_rust(consumer, result.stdout + result.stderr)
    elif consumer.runner == "foundry":
        result = run_command(
            [
                "forge", "test", "--root", "contracts", "--match-path",
                "test/ProtocolVectors.t.sol", "--match-test", consumer.selector, "--json",
            ],
            root,
            runner,
        )
        validate_foundry(consumer, result.stdout)
    elif consumer.runner == "vitest":
        result = run_command(
            [
                "pnpm", "--dir", "ui", "exec", "vitest", "run",
                "src/lib/protocol-vectors.test.ts", "-t", consumer.selector, "--reporter=json",
            ],
            root,
            runner,
        )
        validate_vitest(consumer, result.stdout)
    else:
        raise ValueError(f"unknown refinement runner: {consumer.runner}")


def main() -> int:
    try:
        consumers = parse_manifest(
            json.loads(VECTORS.read_text(encoding="utf-8")),
            MANIFEST.read_text(encoding="utf-8"),
            MODEL.read_text(encoding="utf-8"),
            THEOREMS.read_text(encoding="utf-8"),
            ROOT,
        )
        for consumer in consumers:
            execute_consumer(consumer, ROOT)
            print(f"refinement consumer passed: {consumer.section} {consumer.runner} {consumer.selector}")
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
