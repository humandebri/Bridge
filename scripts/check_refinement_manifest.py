#!/usr/bin/env python3
"""Validate Lean refinement links and execute every registered consumer exactly once."""

from __future__ import annotations

from check_claim_test_manifest import ClaimTest, execute_group, group_tests

import json
import re
from dataclasses import dataclass
from pathlib import Path

from generate_refinement_harness import RENDERERS, Renderer, expected_outputs


ROOT = Path(__file__).resolve().parents[1]
VECTORS = ROOT / "verification" / "generated" / "protocol-vectors.json"
MANIFEST = ROOT / "verification" / "refinement-manifest.tsv"
MODEL = ROOT / "verification" / "lean" / "BridgeSpec" / "Model.lean"
FINITE_WIDTH_MODEL = ROOT / "verification" / "lean" / "BridgeSpec" / "FiniteWidthModel.lean"
MODEL_REFINEMENT = ROOT / "verification" / "lean" / "BridgeSpec" / "ModelRefinement.lean"
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
NON_GENERATED_TESTS = (
    "contracts/test/ProtocolVectors.t.sol",
)


@dataclass(frozen=True)
class Consumer:
    section: str
    abstract_definition: str
    implementation_definition: str
    theorem: str
    runner: str
    target: str
    selector: str


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


def top_level_equality_sides(theorem_source: str, theorem: str) -> tuple[str, str]:
    header = theorem_source.split(":= by", 1)[0]
    marker = re.search(rf"^theorem\s+{re.escape(theorem)}\b", header)
    if marker is None:
        raise ValueError(f"Lean theorem header is malformed: {theorem}")
    depth = 0
    proposition_start: int | None = None
    pairs = {"(": ")", "[": "]", "{": "}"}
    closing = set(pairs.values())
    for index in range(marker.end(), len(header)):
        character = header[index]
        if character in pairs:
            depth += 1
        elif character in closing:
            depth -= 1
            if depth < 0:
                raise ValueError(f"Lean theorem binders are unbalanced: {theorem}")
        elif character == ":" and depth == 0:
            proposition_start = index + 1
            break
    if proposition_start is None:
        raise ValueError(f"Lean theorem proposition is missing: {theorem}")
    proposition = header[proposition_start:]
    depth = 0
    equalities: list[int] = []
    for index, character in enumerate(proposition):
        if character in pairs:
            depth += 1
        elif character in closing:
            depth -= 1
            if depth < 0:
                raise ValueError(f"Lean theorem proposition is unbalanced: {theorem}")
        elif character == "=" and depth == 0:
            equalities.append(index)
    if depth != 0 or len(equalities) != 1:
        raise ValueError(f"Lean theorem {theorem} must have one top-level equality")
    equality = equalities[0]
    return proposition[:equality].strip(), proposition[equality + 1 :].strip()


def parse_manifest(
    document: dict[str, object],
    manifest_text: str,
    model: str,
    implementation: str,
    refinement: str,
    root: Path = ROOT,
    renderers: dict[tuple[str, str], Renderer] = RENDERERS,
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
    identities: set[tuple[str, str]] = set()
    for number, line in enumerate(manifest_text.splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 5 or not all(fields):
            raise ValueError(f"invalid refinement manifest row {number}")
        key = (fields[0], fields[4])
        renderer = renderers.get(key)
        if renderer is None:
            raise ValueError(f"missing refinement renderer: {key}")
        consumer = Consumer(*fields, renderer.target, renderer.selector)
        if not all(
            IDENTIFIER.fullmatch(value)
            for value in (
                consumer.section,
                consumer.abstract_definition,
                consumer.implementation_definition,
                consumer.theorem,
            )
        ):
            raise ValueError(f"invalid refinement identifier in row {number}")
        target = (root / consumer.target).resolve()
        if root.resolve() not in target.parents or not target.is_file():
            raise ValueError(f"refinement consumer is missing: {consumer.target}")
        association = (
            consumer.abstract_definition,
            consumer.implementation_definition,
            consumer.theorem,
        )
        previous = associations.setdefault(consumer.section, association)
        if previous != association:
            raise ValueError(f"conflicting refinement association: {consumer.section}")
        identity = (consumer.section, consumer.runner)
        if identity in identities:
            raise ValueError(f"duplicate refinement consumer: {identity}")
        identities.add(identity)
        consumers.append(consumer)

    if set(associations) != vector_sections:
        raise ValueError(
            f"refinement sections {sorted(associations)} do not match vectors "
            f"{sorted(vector_sections)}"
        )
    if identities != set(renderers):
        raise ValueError(
            f"renderer coverage differs from manifest: "
            f"missing={sorted(set(renderers) - identities)} "
            f"extra={sorted(identities - set(renderers))}"
        )
    for section, (abstract, bounded, theorem) in associations.items():
        declaration(model, "def", abstract)
        declaration(implementation, "def", bounded)
        theorem_source = declaration(refinement, "theorem", theorem)
        left, right = top_level_equality_sides(theorem_source, theorem)
        bounded_left = re.search(rf"\b{re.escape(bounded)}\b", left) is not None
        abstract_right = re.search(rf"\b{re.escape(abstract)}\b", right) is not None
        abstract_left = re.search(rf"\b{re.escape(abstract)}\b", left) is not None
        bounded_right = re.search(rf"\b{re.escape(bounded)}\b", right) is not None
        if not bounded_left or not abstract_right or abstract_left or bounded_right:
            raise ValueError(
                f"Lean theorem {theorem} must place {bounded} on the left and "
                f"{abstract} on the right for {section}"
            )
    return consumers


def validate_generated_selector_ownership(
    consumers: list[Consumer],
    root: Path = ROOT,
    non_generated_tests: tuple[str, ...] = NON_GENERATED_TESTS,
) -> None:
    sources = {
        target: (root / target).read_text(encoding="utf-8")
        for target in non_generated_tests
        if (root / target).is_file()
    }
    for consumer in consumers:
        owners = [
            target
            for target, source in sources.items()
            if re.search(rf"\b{re.escape(consumer.selector)}\b", source)
        ]
        if owners:
            raise ValueError(
                f"generated refinement selector has a non-generated owner: "
                f"{consumer.selector} -> {', '.join(owners)}"
            )


def main() -> int:
    stale = [
        path.relative_to(ROOT).as_posix()
        for path, expected in expected_outputs().items()
        if not path.is_file() or path.read_text(encoding="utf-8") != expected
    ]
    if stale:
        raise ValueError(f"generated refinement harness is stale: {', '.join(stale)}")
    consumers = parse_manifest(
        json.loads(VECTORS.read_text(encoding="utf-8")),
        MANIFEST.read_text(encoding="utf-8"),
        MODEL.read_text(encoding="utf-8"),
        FINITE_WIDTH_MODEL.read_text(encoding="utf-8"),
        MODEL_REFINEMENT.read_text(encoding="utf-8"),
    )
    validate_generated_selector_ownership(consumers)
    tests = [ClaimTest(
        "rust-core" if consumer.runner == "rust" else consumer.runner,
        consumer.target, consumer.selector, consumer.selector,
    ) for consumer in consumers]
    for group in group_tests(tests):
        execute_group(group)
        for test in group:
            print(f"refinement consumer passed: {test.runner} {test.selector}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
