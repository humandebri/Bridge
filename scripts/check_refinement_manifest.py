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
IMPLEMENTATIONS = ROOT / "verification" / "lean" / "BridgeSpec" / "Implementation.lean"
REFINEMENTS = ROOT / "verification" / "lean" / "BridgeSpec" / "Refinement.lean"
CLAIMS = ROOT / "verification" / "lean" / "BridgeSpec" / "Claims.lean"
PROTOCOL = ROOT / "verification" / "lean" / "BridgeSpec" / "Protocol.lean"
CLAIM_MANIFEST = ROOT / "verification" / "phase5-claims.tsv"
ASSUMPTIONS = ROOT / "verification" / "assumptions.tsv"
VERUS_MANIFEST = ROOT / "verification" / "verus" / "manifest.tsv"
VERUS_PASS = ROOT / "verification" / "verus" / "pass.rs"
PROOF_LINKS = ROOT / "canister" / "bridge-core" / "tests" / "proof_links.rs"
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


@dataclass(frozen=True)
class Claim:
    claim_id: str
    abstract_theorem: str
    bounded_refinement_theorem: str
    trace_theorems: str
    verus_obligations: str
    section: str
    production_links: str
    transaction_tests: str
    assumption_ids: str
    abstract_evidence: str
    bounded_evidence: str
    trace_evidence: str
    verus_evidence: str
    production_evidence: str
    external_evidence: str
    phase_gate: str
    status: str


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


def checked_repository_file(root: Path, relative_text: str) -> Path:
    relative = Path(relative_text)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"claim production path must stay inside the repository: {relative_text}")
    path = (root / relative).resolve()
    if root.resolve() not in path.parents or not path.is_file():
        raise ValueError(f"claim production source is missing: {relative_text}")
    return path


def parse_assumptions(text: str, root: Path) -> dict[str, set[str]]:
    assumptions: dict[str, set[str]] = {}
    for number, line in enumerate(text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 6 or not all(fields):
            raise ValueError(f"invalid external assumption row {number}")
        assumption, _, dependent_claims, validation_links, _, _ = fields
        if not IDENTIFIER.fullmatch(assumption) or assumption in assumptions:
            raise ValueError(f"invalid or duplicate external assumption: {assumption}")
        dependencies = set(dependent_claims.split(";"))
        if not dependencies or any(not IDENTIFIER.fullmatch(value) for value in dependencies):
            raise ValueError(f"invalid dependent claim for external assumption: {assumption}")
        for link in validation_links.split(";"):
            if link.count("#") != 1:
                raise ValueError(f"invalid assumption validation link: {link}")
            path_text, selector = link.split("#")
            if not IDENTIFIER.fullmatch(selector):
                raise ValueError(f"invalid assumption validation selector: {selector}")
            source = checked_repository_file(root, path_text).read_text(encoding="utf-8")
            if re.search(rf"\b{re.escape(selector)}\b", source) is None:
                raise ValueError(f"missing assumption validation selector: {link}")
        assumptions[assumption] = dependencies
    if not assumptions:
        raise ValueError("external assumption registry is empty")
    return assumptions


def parse_claims(
    text: str,
    assumptions: dict[str, set[str]],
    claims_source: str,
    refinements_source: str,
    protocol_source: str,
    verus_manifest_text: str,
    proof_links_source: str,
    root: Path,
) -> list[Claim]:
    claims: list[Claim] = []
    seen_ids: set[str] = set()
    seen_abstract: set[str] = set()
    seen_refinement: set[str] = set()
    seen_trace: set[str] = set()
    seen_sections: set[str] = set()
    actual_assumption_dependencies = {assumption: set() for assumption in assumptions}
    verus_rows: dict[str, list[tuple[str, str]]] = {}
    seen_verus_entries: set[tuple[str, str, str]] = set()
    for number, line in enumerate(verus_manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 5:
            raise ValueError(f"invalid Verus manifest row {number}")
        kind, kernel_name, proof_name, _, _ = fields
        if kind not in {"shared", "model", "executable"}:
            raise ValueError(f"invalid Verus evidence kind: {kind}")
        entry = (kind, kernel_name, proof_name)
        if entry in seen_verus_entries:
            raise ValueError(f"duplicate Verus manifest entry: {proof_name}")
        seen_verus_entries.add(entry)
        verus_rows.setdefault(proof_name, []).append((kind, kernel_name))
    verus_proofs = set(verus_rows)
    verus_pass_source = (root / "verification/verus/pass.rs").read_text(encoding="utf-8")
    registered_links = re.findall(
        r'production_link!\(\s*"([A-Za-z_][A-Za-z0-9_]*)",\s*"([^"]+#'
        r'[A-Za-z_][A-Za-z0-9_]*)"',
        proof_links_source,
    )
    if len(registered_links) != len(set(registered_links)):
        raise ValueError("duplicate typed production link registry entry")
    registered_rust_links = set(registered_links)
    claimed_rust_links: set[tuple[str, str]] = set()
    used_verus_proofs: set[str] = set()
    for number, line in enumerate(text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 17 or not all(fields):
            raise ValueError(f"invalid Phase 5 claim row {number}")
        claim = Claim(*fields)
        identifiers = (
            claim.claim_id,
            claim.abstract_theorem,
            claim.bounded_refinement_theorem,
            claim.section,
        )
        if not all(IDENTIFIER.fullmatch(value) for value in identifiers):
            raise ValueError(f"invalid identifier in Phase 5 claim row {number}")
        if (
            claim.claim_id in seen_ids
            or claim.abstract_theorem in seen_abstract
            or claim.bounded_refinement_theorem in seen_refinement
            or claim.section in seen_sections
        ):
            raise ValueError(f"duplicate Phase 5 claim mapping in row {number}")
        seen_ids.add(claim.claim_id)
        seen_abstract.add(claim.abstract_theorem)
        seen_refinement.add(claim.bounded_refinement_theorem)
        seen_sections.add(claim.section)
        trace_theorems = [] if claim.trace_theorems == "-" else claim.trace_theorems.split(";")
        if claim.status == "complete" and not trace_theorems:
            raise ValueError(f"complete claim lacks a trace theorem: {claim.claim_id}")
        if (
            any(not IDENTIFIER.fullmatch(theorem) for theorem in trace_theorems)
            or len(trace_theorems) != len(set(trace_theorems))
            or seen_trace.intersection(trace_theorems)
        ):
            raise ValueError(f"invalid or duplicate trace theorem in row {number}")
        seen_trace.update(trace_theorems)
        if claim.phase_gate not in {"phase1", "phase2", "phase3", "phase4", "phase5"}:
            raise ValueError(f"invalid phase gate for {claim.claim_id}")
        if claim.status != "complete":
            raise ValueError(f"incomplete Phase 5 claim: {claim.claim_id}")
        obligations = [] if claim.verus_obligations == "-" else claim.verus_obligations.split(";")
        if claim.status == "complete" and not obligations:
            raise ValueError(f"complete claim lacks a Verus obligation: {claim.claim_id}")
        unknown_proofs = set(obligations) - verus_proofs
        if unknown_proofs:
            raise ValueError(
                f"unknown Verus obligation for {claim.claim_id}: {sorted(unknown_proofs)}"
            )
        used_verus_proofs.update(obligations)
        has_executable_obligation = any(
            kind == "executable"
            for obligation in obligations
            for kind, _ in verus_rows.get(obligation, [])
        )
        expected_verus_evidence = (
            "executable-proved" if has_executable_obligation else "proved"
        )
        expected_evidence = {
            "abstract": (claim.abstract_evidence, "proved"),
            "bounded": (claim.bounded_evidence, "proved"),
            "trace": (claim.trace_evidence, "proved"),
            "verus": (claim.verus_evidence, expected_verus_evidence),
            "production": (claim.production_evidence, "refinement-tested"),
            "external": (claim.external_evidence, "assumed"),
        }
        mismatches = [
            f"{kind}={actual} (expected {expected})"
            for kind, (actual, expected) in expected_evidence.items()
            if actual != expected
        ]
        if mismatches:
            raise ValueError(
                f"claim evidence mismatch for {claim.claim_id}: {', '.join(mismatches)}"
            )
        for obligation in obligations:
            executable_rows = [
                kernel_name
                for kind, kernel_name in verus_rows[obligation]
                if kind == "executable"
            ]
            if not executable_rows:
                continue
            if len(executable_rows) != 1:
                raise ValueError(f"ambiguous executable Verus obligation: {obligation}")
            kernel_name = executable_rows[0]
            match = re.search(
                rf"^fn {re.escape(obligation)}\b(?P<body>.*?)(?=^(?:proof )?fn |\Z)",
                verus_pass_source,
                re.MULTILINE | re.DOTALL,
            )
            if match is None:
                raise ValueError(f"missing executable Verus obligation: {obligation}")
            body = match.group("body")
            if (
                re.search(rf"\bkernel::{re.escape(kernel_name)}\s*\(", body) is None
                or re.search(rf"\b{re.escape(kernel_name)}_spec\b", body) is not None
            ):
                raise ValueError(
                    f"executable Verus obligation {obligation} must call "
                    f"the registered production function {kernel_name}"
                )
        claim_assumptions = claim.assumption_ids.split(";")
        unknown = set(claim_assumptions) - set(assumptions)
        if not claim_assumptions or unknown:
            raise ValueError(
                f"unknown external assumption for {claim.claim_id}: {sorted(unknown)}"
            )
        for assumption in claim_assumptions:
            actual_assumption_dependencies[assumption].add(claim.claim_id)
        for link in claim.production_links.split(";"):
            if link.count("#") != 1:
                raise ValueError(f"invalid production link for {claim.claim_id}: {link}")
            path_text, symbol = link.split("#")
            if not IDENTIFIER.fullmatch(symbol):
                raise ValueError(f"invalid production symbol for {claim.claim_id}: {symbol}")
            source = checked_repository_file(root, path_text).read_text(encoding="utf-8")
            if re.search(rf"\b{re.escape(symbol)}\b", source) is None:
                raise ValueError(f"missing production symbol for {claim.claim_id}: {link}")
            if path_text.endswith(".rs"):
                claimed_rust_links.add((claim.claim_id, link))
        for link in claim.transaction_tests.split(";"):
            if link.count("#") != 1:
                raise ValueError(f"invalid transaction test for {claim.claim_id}: {link}")
            path_text, selector = link.split("#")
            if not IDENTIFIER.fullmatch(selector):
                raise ValueError(f"invalid transaction test selector for {claim.claim_id}: {selector}")
            source = checked_repository_file(root, path_text).read_text(encoding="utf-8")
            if re.search(rf"\b{re.escape(selector)}\b", source) is None:
                raise ValueError(f"missing transaction test for {claim.claim_id}: {link}")
        claims.append(claim)

    if used_verus_proofs != verus_proofs:
        raise ValueError(
            f"claim Verus obligations {sorted(used_verus_proofs)} do not match "
            f"Verus manifest {sorted(verus_proofs)}"
        )
    if claimed_rust_links != registered_rust_links:
        raise ValueError(
            f"typed production links {sorted(registered_rust_links)} do not match "
            f"Rust claim links {sorted(claimed_rust_links)}"
        )

    declared_claims = set(
        re.findall(r"^theorem ([A-Za-z_][A-Za-z0-9_]*)\b", claims_source, re.MULTILINE)
    )
    declared_refinements = set(
        re.findall(r"^theorem ([A-Za-z_][A-Za-z0-9_]*)\b", refinements_source, re.MULTILINE)
    )
    declared_trace = set(
        re.findall(r"^theorem ([A-Za-z_][A-Za-z0-9_]*)\b", protocol_source, re.MULTILINE)
    )
    if declared_claims != seen_abstract:
        raise ValueError(
            f"Phase 5 abstract theorems {sorted(seen_abstract)} do not match "
            f"Claims.lean {sorted(declared_claims)}"
        )
    if declared_refinements != seen_refinement:
        raise ValueError(
            f"Phase 5 refinement theorems {sorted(seen_refinement)} do not match "
            f"Refinement.lean {sorted(declared_refinements)}"
        )
    if declared_trace != seen_trace:
        raise ValueError(
            f"Phase 5 trace theorems {sorted(seen_trace)} do not match "
            f"Protocol.lean {sorted(declared_trace)}"
        )
    validate_protocol_evidence(protocol_source)
    for assumption, declared_dependencies in assumptions.items():
        actual_dependencies = actual_assumption_dependencies[assumption]
        if declared_dependencies != actual_dependencies:
            raise ValueError(
                f"external assumption dependencies for {assumption} "
                f"{sorted(declared_dependencies)} do not match claims "
                f"{sorted(actual_dependencies)}"
            )
    return claims


def validate_protocol_evidence(protocol_source: str) -> None:
    step_match = re.search(
        r"^def step\b(?P<body>.*?)(?=^(?:def|theorem|inductive|structure|end)\b|\Z)",
        protocol_source,
        re.MULTILINE | re.DOTALL,
    )
    if step_match is None or re.search(r"\brawStep\b", step_match.group("body")) is None:
        raise ValueError("Protocol step must delegate to rawStep")
    if re.search(r"\bif\s+Safe\b", step_match.group("body")) is not None:
        raise ValueError("Protocol step must not filter a raw transition through Safe")

    raw_preservation = declaration(
        protocol_source, "theorem", "raw_step_preserves_safe"
    )
    for required in ("Safe state", "rawStep state event = some next", "Safe next"):
        if required not in raw_preservation:
            raise ValueError(
                "raw_step_preserves_safe must prove rawStep preservation directly"
            )
    step_preservation = declaration(
        protocol_source, "theorem", "step_preserves_safe"
    )
    if "raw_step_preserves_safe" not in step_preservation:
        raise ValueError("step_preserves_safe must be derived from raw-step preservation")

    reachability = declaration(
        protocol_source,
        "theorem",
        "conditional_committed_withdrawal_reaches_paid",
    )
    for required in (
        "canonicalValid",
        "cycles",
        "fair",
        ".observeCanonical",
        ".executorClaim",
        ".settle",
    ):
        if required not in reachability:
            raise ValueError(
                "conditional reachability must consume certificates and construct "
                "the production event sequence"
            )


def parse_manifest(
    document: dict[str, object],
    manifest_text: str,
    model: str,
    implementations: str,
    refinements: str,
    claims_source: str,
    protocol_source: str,
    claim_manifest_text: str,
    assumptions_text: str,
    verus_manifest_text: str,
    proof_links_source: str,
    root: Path,
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
    associations: dict[str, tuple[str, str, str]] = {}
    seen_consumers: set[tuple[str, str, str]] = set()
    for number, line in enumerate(manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 7 or not all(fields):
            raise ValueError(f"invalid refinement manifest row {number}")
        consumer = Consumer(*fields)
        if not all(IDENTIFIER.fullmatch(value) for value in (
            consumer.section,
            consumer.abstract_definition,
            consumer.implementation_definition,
            consumer.theorem,
            consumer.selector,
        )):
            raise ValueError(f"invalid refinement identifier in row {number}")
        association = (
            consumer.abstract_definition,
            consumer.implementation_definition,
            consumer.theorem,
        )
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
    for section, (definition, implementation, theorem) in associations.items():
        declaration(model, "def", definition)
        declaration(implementations, "def", implementation)
        theorem_source = declaration(refinements, "theorem", theorem)
        theorem_statement = theorem_source.split(":= by", 1)[0]
        if (
            re.search(rf"\b{re.escape(definition)}\b", theorem_statement) is None
            or re.search(rf"\b{re.escape(implementation)}\b", theorem_statement) is None
        ):
            raise ValueError(
                f"Lean theorem {theorem} does not directly relate "
                f"{definition} and {implementation} for {section}"
            )
    assumptions = parse_assumptions(assumptions_text, root)
    claims = parse_claims(
        claim_manifest_text, assumptions, claims_source, refinements, protocol_source,
        verus_manifest_text, proof_links_source, root
    )
    claims_by_section = {claim.section: claim for claim in claims}
    if set(claims_by_section) != vector_sections:
        raise ValueError(
            f"Phase 5 claim sections {sorted(claims_by_section)} do not match "
            f"vectors {sorted(vector_sections)}"
        )
    for section, (_, _, theorem) in associations.items():
        claim = claims_by_section[section]
        if claim.bounded_refinement_theorem != theorem:
            raise ValueError(f"claim refinement theorem mismatch for {section}")
    registered_refinements = {consumer.theorem for consumer in consumers}
    claim_refinements = {claim.bounded_refinement_theorem for claim in claims}
    if claim_refinements != registered_refinements:
        raise ValueError(
            f"refinement manifest theorems {sorted(registered_refinements)} do not match "
            f"Phase 5 claims {sorted(claim_refinements)}"
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


def execute_proof_links(root: Path, runner: CommandRunner = subprocess.run) -> None:
    result = run_command(
        ["cargo", "test", "--locked", "-p", "bridge-core", "--test", "proof_links"],
        root,
        runner,
    )
    output = result.stdout + result.stderr
    if (
        "test phase5_production_links_typecheck ... ok" not in output
        or "test result: ok. 1 passed; 0 failed;" not in output
    ):
        raise ValueError("compiled Phase 5 production proof links did not pass exactly once")


def execute_transaction_test(
    path_text: str,
    selector: str,
    root: Path,
    runner: CommandRunner = subprocess.run,
) -> None:
    if path_text.startswith("canister/bridge-core/tests/"):
        target = Path(path_text).stem
        result = run_command(
            [
                "cargo", "test", "--locked", "-p", "bridge-core", "--test", target,
                selector, "--", "--exact",
            ],
            root,
            runner,
        )
        output = result.stdout + result.stderr
        expected = rf"^test {re.escape(selector)} \.\.\. ok$"
    elif path_text.startswith("canister/bridge-canister/src/"):
        result = run_command(
            ["cargo", "test", "--locked", "-p", "bridge-canister", selector],
            root,
            runner,
        )
        output = result.stdout + result.stderr
        expected = rf"^test .*::{re.escape(selector)} \.\.\. ok$"
    elif path_text.startswith("ui/src/"):
        result = run_command(
            [
                "pnpm", "--dir", "ui", "exec", "vitest", "run", path_text.removeprefix("ui/"),
                "-t", selector, "--reporter=json",
            ],
            root,
            runner,
        )
        report = json.loads(result.stdout)
        matches = [
            assertion
            for test in report.get("testResults", [])
            for assertion in test.get("assertionResults", [])
            if assertion.get("title") == selector and assertion.get("status") == "passed"
        ]
        if report.get("numPassedTests") != 1 or len(matches) != 1:
            raise ValueError(f"transaction test did not execute exactly once: {selector}")
        return
    else:
        raise ValueError(f"unsupported transaction test target: {path_text}")
    if len(re.findall(r"^running 1 test$", output, re.MULTILINE)) != 1:
        raise ValueError(f"transaction test did not execute exactly once: {selector}")
    if len(re.findall(expected, output, re.MULTILINE)) != 1:
        raise ValueError(f"transaction test did not pass exactly once: {selector}")


def main() -> int:
    try:
        consumers = parse_manifest(
            json.loads(VECTORS.read_text(encoding="utf-8")),
            MANIFEST.read_text(encoding="utf-8"),
            MODEL.read_text(encoding="utf-8"),
            IMPLEMENTATIONS.read_text(encoding="utf-8"),
            REFINEMENTS.read_text(encoding="utf-8"),
            CLAIMS.read_text(encoding="utf-8"),
            PROTOCOL.read_text(encoding="utf-8"),
            CLAIM_MANIFEST.read_text(encoding="utf-8"),
            ASSUMPTIONS.read_text(encoding="utf-8"),
            VERUS_MANIFEST.read_text(encoding="utf-8"),
            PROOF_LINKS.read_text(encoding="utf-8"),
            ROOT,
        )
        execute_proof_links(ROOT)
        print("compiled production proof links passed")
        transaction_tests = {
            tuple(link.split("#"))
            for line in CLAIM_MANIFEST.read_text(encoding="utf-8").splitlines()
            for link in line.split("\t")[7].split(";")
        }
        for path_text, selector in sorted(transaction_tests):
            execute_transaction_test(path_text, selector, ROOT)
            print(f"production transaction test passed: {selector}")
        for consumer in consumers:
            execute_consumer(consumer, ROOT)
            print(f"refinement consumer passed: {consumer.section} {consumer.runner} {consumer.selector}")
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
