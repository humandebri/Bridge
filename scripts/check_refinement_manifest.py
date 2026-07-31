#!/usr/bin/env python3
"""Validate Lean refinement links and execute every registered consumer exactly once."""

from __future__ import annotations

import json
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Sequence

import check_claim_test_manifest


ROOT = Path(__file__).resolve().parents[1]
VECTORS = ROOT / "verification" / "generated" / "protocol-vectors.json"
MANIFEST = ROOT / "verification" / "refinement-manifest.tsv"
MODEL = ROOT / "verification" / "lean" / "BridgeSpec" / "Model.lean"
IMPLEMENTATION = ROOT / "verification" / "lean" / "BridgeSpec" / "Implementation.lean"
REFINEMENT = ROOT / "verification" / "lean" / "BridgeSpec" / "Refinement.lean"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
RUNNER_TARGETS = {
    ("rust", "canister/bridge-core/tests/protocol_vectors.rs"),
    ("foundry", "contracts/test/ProtocolVectors.t.sol"),
    ("vitest", "ui/src/lib/protocol-vectors.test.ts"),
}


@dataclass(frozen=True)
class Consumer:
    section: str
    abstract_definition: str
    implementation_definition: str
    theorem: str
    runner: str
    target: str
    selector: str
    production_symbol: str


CommandRunner = Callable[..., subprocess.CompletedProcess[str]]


def declaration(source: str, keyword: str, name: str) -> str:
    start = re.search(rf"^{keyword} {re.escape(name)}\b", source, re.MULTILINE)
    if start is None:
        raise ValueError(f"Lean {keyword} is missing: {name}")
    following = re.search(
        r"^(?:def|theorem|inductive|structure|end)\b",
        source[start.end() :],
        re.MULTILINE,
    )
    end = len(source) if following is None else start.end() + following.start()
    return source[start.start() : end]


def consumer_source(source: str, consumer: Consumer) -> str:
    if consumer.runner == "rust":
        start_pattern = rf"(?m)^fn\s+{re.escape(consumer.selector)}\s*\("
        end_pattern = r"(?m)^fn\s+[A-Za-z_][A-Za-z0-9_]*\s*\("
    elif consumer.runner == "foundry":
        start_pattern = (
            rf"(?m)^[ \t]+function\s+{re.escape(consumer.selector)}\s*\("
        )
        end_pattern = r"(?m)^[ \t]+function\s+[A-Za-z_][A-Za-z0-9_]*\s*\("
    elif consumer.runner == "vitest":
        start_pattern = (
            rf"""(?mx)^[ \t]+(?:it|test)\s*\(\s*
            (?P<quote>["']){re.escape(consumer.selector)}(?P=quote)\s*,
            """
        )
        end_pattern = r"(?m)^(?:[ \t]+(?:it|test)\s*\(|\}\)\s*;?\s*$)"
    else:
        raise ValueError(f"unknown refinement runner: {consumer.runner}")

    start = re.search(start_pattern, source)
    if start is None:
        raise ValueError(f"refinement selector is missing: {consumer.selector}")
    following = re.search(end_pattern, source[start.end() :])
    end = len(source) if following is None else start.end() + following.start()
    return source[start.start() : end]


def validate_consumer_binding(consumer: Consumer, source: str) -> None:
    body = consumer_source(source, consumer)
    if consumer.runner == "rust":
        section_pattern = (
            rf"\bvectors\s*\(\s*\)\s*\.\s*{re.escape(consumer.section)}\b"
        )
    elif consumer.runner == "vitest":
        section_pattern = rf"\bvectors\s*\.\s*{re.escape(consumer.section)}\b"
    else:
        section_pattern = rf"""["']\.{re.escape(consumer.section)}\[[\"']"""
    if re.search(section_pattern, body) is None:
        raise ValueError(
            f"refinement consumer does not consume section: "
            f"{consumer.selector} -> {consumer.section}"
        )
    if re.search(
        rf"\b{re.escape(consumer.production_symbol)}\s*\(", body
    ) is None:
        raise ValueError(
            f"refinement consumer does not call production symbol: "
            f"{consumer.selector} -> {consumer.production_symbol}"
        )


def parse_manifest(
    document: dict[str, object],
    manifest_text: str,
    model: str,
    implementation: str,
    refinement: str,
    root: Path = ROOT,
) -> list[Consumer]:
    if document.get("schema_version") != 3:
        raise ValueError("protocol vector schema must be exactly v3")
    vector_sections = {
        key
        for key, value in document.items()
        if key.endswith("_cases") and isinstance(value, list)
    }
    if any(not document[section] for section in vector_sections):
        raise ValueError("every protocol vector section must be nonempty")

    consumers: list[Consumer] = []
    associations: dict[str, tuple[str, str, str]] = {}
    identities: set[tuple[str, str, str]] = set()
    for number, line in enumerate(manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 8 or not all(fields):
            raise ValueError(f"invalid refinement manifest row {number}")
        consumer = Consumer(*fields)
        if not all(
            IDENTIFIER.fullmatch(value)
            for value in (
                consumer.section,
                consumer.abstract_definition,
                consumer.implementation_definition,
                consumer.theorem,
                consumer.selector,
                consumer.production_symbol,
            )
        ):
            raise ValueError(f"invalid refinement identifier in row {number}")
        if (consumer.runner, consumer.target) not in RUNNER_TARGETS:
            raise ValueError(
                f"unsupported refinement runner target: {consumer.runner} {consumer.target}"
            )
        target = (root / consumer.target).resolve()
        if root.resolve() not in target.parents or not target.is_file():
            raise ValueError(f"refinement consumer is missing: {consumer.target}")
        validate_consumer_binding(
            consumer,
            target.read_text(encoding="utf-8"),
        )
        association = (
            consumer.abstract_definition,
            consumer.implementation_definition,
            consumer.theorem,
        )
        previous = associations.setdefault(consumer.section, association)
        if previous != association:
            raise ValueError(f"conflicting refinement association: {consumer.section}")
        identity = (consumer.runner, consumer.target, consumer.selector)
        if identity in identities:
            raise ValueError(f"duplicate refinement consumer: {identity}")
        identities.add(identity)
        consumers.append(consumer)

    if set(associations) != vector_sections:
        raise ValueError(
            f"refinement sections {sorted(associations)} do not match vectors "
            f"{sorted(vector_sections)}"
        )
    for section, (abstract, bounded, theorem) in associations.items():
        declaration(model, "def", abstract)
        declaration(implementation, "def", bounded)
        theorem_source = declaration(refinement, "theorem", theorem)
        statement = theorem_source.split(":= by", 1)[0]
        if not re.search(rf"\b{re.escape(abstract)}\b", statement) or not re.search(
            rf"\b{re.escape(bounded)}\b", statement
        ):
            raise ValueError(
                f"Lean theorem {theorem} does not relate {abstract} and {bounded} "
                f"for {section}"
            )
    return consumers


def run_command(
    command: Sequence[str],
    root: Path,
    runner: CommandRunner = subprocess.run,
) -> subprocess.CompletedProcess[str]:
    result = runner(command, cwd=root, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise ValueError(
            f"refinement consumer failed: {' '.join(command)}\n"
            f"{result.stdout}{result.stderr}"
        )
    return result


def execute_consumer(
    consumer: Consumer,
    root: Path = ROOT,
    runner: CommandRunner = subprocess.run,
) -> None:
    if consumer.runner == "rust":
        result = run_command(
            [
                "cargo",
                "test",
                "--locked",
                "-p",
                "bridge-core",
                "--test",
                "protocol_vectors",
                consumer.selector,
                "--",
                "--exact",
            ],
            root,
            runner,
        )
        output = result.stdout + result.stderr
        if len(re.findall(r"^running 1 test$", output, re.MULTILINE)) != 1 or len(
            re.findall(
                rf"^test {re.escape(consumer.selector)} \.\.\. ok$",
                output,
                re.MULTILINE,
            )
        ) != 1:
            raise ValueError(
                f"Rust refinement consumer did not pass exactly once: {consumer.selector}"
            )
    elif consumer.runner == "foundry":
        result = run_command(
            [
                "forge",
                "test",
                "--root",
                "contracts",
                "--match-path",
                "test/ProtocolVectors.t.sol",
                "--match-test",
                consumer.selector,
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
            or results[0][0] != f"{consumer.selector}()"
            or results[0][1].get("status") != "Success"
        ):
            raise ValueError(
                f"Foundry refinement consumer did not pass exactly once: {consumer.selector}"
            )
    elif consumer.runner == "vitest":
        result = run_command(
            [
                "pnpm",
                "--dir",
                "ui",
                "exec",
                "vitest",
                "run",
                "src/lib/protocol-vectors.test.ts",
                "-t",
                consumer.selector,
                "--reporter=json",
            ],
            root,
            runner,
        )
        report = json.loads(result.stdout)
        matches = [
            assertion
            for test in report.get("testResults", [])
            for assertion in test.get("assertionResults", [])
            if assertion.get("title") == consumer.selector
            and assertion.get("status") == "passed"
        ]
        if report.get("numPassedTests") != 1 or len(matches) != 1:
            raise ValueError(
                f"Vitest refinement consumer did not pass exactly once: {consumer.selector}"
            )
    else:
        raise ValueError(f"unknown refinement runner: {consumer.runner}")


def main() -> int:
    consumers = parse_manifest(
        json.loads(VECTORS.read_text(encoding="utf-8")),
        MANIFEST.read_text(encoding="utf-8"),
        MODEL.read_text(encoding="utf-8"),
        IMPLEMENTATION.read_text(encoding="utf-8"),
        REFINEMENT.read_text(encoding="utf-8"),
    )
    for consumer in consumers:
        execute_consumer(consumer)
        print(
            f"refinement consumer passed: {consumer.section} "
            f"{consumer.runner} {consumer.selector}"
        )
    check_claim_test_manifest.main()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
