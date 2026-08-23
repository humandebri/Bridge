#!/usr/bin/env python3
"""Validate Verus proof strength, shared expressions, and exact production call sites."""

from __future__ import annotations

import re
from pathlib import Path

from check_transition_manifest import production_body_calls, strip_comments_and_strings
from verus_manifest import parse_verus_manifest


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "verification" / "verus" / "manifest.tsv"
PASS = ROOT / "verification" / "verus" / "pass.rs"
FAIL = ROOT / "verification" / "verus" / "fail"
KERNEL = ROOT / "canister" / "bridge-core" / "src" / "kernel.rs"
PRODUCTION_ROOTS = (
    (ROOT / "canister" / "bridge-core" / "src").resolve(),
    (ROOT / "canister" / "bridge-canister" / "src").resolve(),
)


def verus_spec_body(source: str, name: str) -> str:
    cleaned = strip_comments_and_strings(source)
    marker = re.search(rf"\bpub\s+open\s+spec\s+fn\s+{re.escape(name)}\s*\(", cleaned)
    if marker is None:
        raise ValueError(f"missing Verus spec body: {name}")
    brace = cleaned.find("{", marker.end())
    if brace < 0:
        raise ValueError(f"Verus spec has no body: {name}")
    depth = 0
    for index in range(brace, len(cleaned)):
        if cleaned[index] == "{":
            depth += 1
        elif cleaned[index] == "}":
            depth -= 1
            if depth == 0:
                return cleaned[brace : index + 1]
    raise ValueError(f"unbalanced Verus spec body: {name}")


def rust_body(cleaned: str, name: str) -> str:
    function_marker = re.compile(
        rf"\b(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+{re.escape(name)}\s*\("
    )
    marker = function_marker.search(cleaned)
    if marker is None:
        raise ValueError(f"missing production call-site function: {name}")
    next_function = re.compile(
        r"\b(?:(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+"
        r"\w+(?:<[^>{}]*>)?\s*\(|(?:pub\s+)?(?:struct|enum)\s+\w+)"
    ).search(cleaned, marker.end())
    limit = next_function.start() if next_function else len(cleaned)
    depth = 0
    start = None
    candidates: list[tuple[int, int]] = []
    for index in range(marker.end(), limit):
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
    raise ValueError(f"unbalanced production call-site body: {name}")


def verus_function_definition(
    cleaned: str, name: str, *, proof_function: bool
) -> str:
    prefix = r"(?:pub\s+)?proof\s+fn" if proof_function else r"(?:pub\s+)?fn"
    marker = re.compile(rf"\b{prefix}\s+{re.escape(name)}\s*\(")
    matches = list(marker.finditer(cleaned))
    if len(matches) != 1:
        raise ValueError(
            f"Verus function definition must resolve exactly once: {name}/{len(matches)}"
        )
    match = matches[0]
    next_function = re.compile(
        r"\b(?:(?:proof|pub|open|spec|const|async)\s+)*fn\s+"
        r"[A-Za-z_][A-Za-z0-9_]*\s*\("
    ).search(cleaned, match.end())
    return cleaned[match.start() : next_function.start() if next_function else len(cleaned)]


def _definition_body(definition: str, name: str) -> str:
    depth = 0
    start = None
    candidates: list[tuple[int, int]] = []
    for index, char in enumerate(definition):
        if char == "{":
            if depth == 0:
                start = index
            depth += 1
        elif char == "}":
            if depth == 0:
                raise ValueError(f"unbalanced Verus function body: {name}")
            depth -= 1
            if depth == 0:
                assert start is not None
                candidates.append((start, index + 1))
    if depth or not candidates:
        raise ValueError(f"unbalanced Verus function body: {name}")
    start, end = candidates[-1]
    return definition[start:end]


def _ensures_contract(definition: str, body: str, name: str) -> str:
    body_start = definition.rfind(body)
    contract = definition[:body_start]
    marker = re.search(r"\bensures\b", contract)
    if marker is None:
        raise ValueError(f"Verus function has no ensures contract: {name}")
    end_marker = re.search(r"\bdecreases\b", contract[marker.end() :])
    end = (
        marker.end() + end_marker.start()
        if end_marker is not None
        else len(contract)
    )
    return contract[marker.end() : end]


def _tail_expression(body: str, name: str) -> str:
    inner = body[1:-1]
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    last_statement = -1
    for index, char in enumerate(inner):
        if char in "([{":
            stack.append(char)
        elif char in pairs:
            if not stack or stack.pop() != pairs[char]:
                raise ValueError(f"unbalanced Verus executable body: {name}")
        elif char == ";" and not stack:
            last_statement = index
    if stack:
        raise ValueError(f"unbalanced Verus executable body: {name}")
    tail = inner[last_statement + 1 :].strip()
    if not tail:
        raise ValueError(f"Verus executable proof must return a tail expression: {name}")
    return tail


def validate_proof_binding(
    cleaned_pass: str, kind: str, kernel: str, proof: str
) -> None:
    definition = verus_function_definition(
        cleaned_pass, proof, proof_function=kind != "executable"
    )
    body = _definition_body(definition, proof)
    ensures = _ensures_contract(definition, body, proof)
    if kind == "executable":
        result = re.search(r"->\s*\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*:", definition)
        if result is None or re.search(rf"\b{re.escape(result.group(1))}\b", ensures) is None:
            raise ValueError(
                f"Verus executable proof result is not constrained by ensures: {proof}"
            )
        tail = _tail_expression(body, proof)
        call = re.compile(rf"\bkernel\s*::\s*{re.escape(kernel)}\s*\(")
        body_calls = len(call.findall(body))
        tail_calls = len(call.findall(tail))
        if body_calls == 0 or body_calls != tail_calls:
            raise ValueError(
                f"Verus executable proof does not return every registered kernel call: "
                f"{proof}/{kernel}"
            )
        return
    spec = f"{kernel}_spec"
    if re.search(rf"\bkernel\s*::\s*{re.escape(spec)}\s*\(", ensures) is None:
        raise ValueError(
            f"Verus proof ensures does not reference registered spec: {proof}/{spec}"
        )


def production_call_site_path(path_text: str) -> Path:
    path = (ROOT / path_text).resolve()
    if (
        path.suffix != ".rs"
        or not any(path.is_relative_to(root) for root in PRODUCTION_ROOTS)
        or not path.is_file()
    ):
        raise ValueError(f"production call-site is outside Rust production roots: {path_text}")
    return path


def _balanced(source: str, start: int, opening: str, closing: str) -> tuple[str, int]:
    if start >= len(source) or source[start] != opening:
        raise ValueError(f"expected {opening} at offset {start}")
    depth = 0
    for index in range(start, len(source)):
        if source[index] == opening:
            depth += 1
        elif source[index] == closing:
            depth -= 1
            if depth == 0:
                return source[start + 1 : index], index + 1
    raise ValueError(f"unbalanced {opening}{closing} expression")


def _split_top_level(source: str) -> tuple[str, ...]:
    items: list[str] = []
    start = 0
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    for index, char in enumerate(source):
        if char in "([{":
            stack.append(char)
        elif char in pairs:
            if not stack or stack.pop() != pairs[char]:
                raise ValueError("unbalanced shared-expression argument")
        elif char == "," and not stack:
            items.append(source[start:index].strip())
            start = index + 1
    if stack:
        raise ValueError("unbalanced shared-expression argument")
    final = source[start:].strip()
    if final:
        items.append(final)
    return tuple(items)


def _macro_invocations(body: str, macro: str) -> tuple[tuple[str, ...], ...]:
    marker = re.compile(rf"\b{re.escape(macro)}\s*!\s*\(")
    invocations: list[tuple[str, ...]] = []
    for match in marker.finditer(body):
        arguments, _ = _balanced(body, match.end() - 1, "(", ")")
        invocations.append(_split_top_level(arguments))
    return tuple(invocations)


def _function_parameters(cleaned: str, name: str, *, specification: bool) -> tuple[str, ...]:
    prefix = r"\bpub\s+open\s+spec\s+fn" if specification else r"\b(?:pub\s+)?(?:const\s+)?fn"
    marker = re.search(rf"{prefix}\s+{re.escape(name)}\s*\(", cleaned)
    if marker is None:
        raise ValueError(f"missing shared-expression function parameters: {name}")
    parameters, _ = _balanced(cleaned, marker.end() - 1, "(", ")")
    names: list[str] = []
    for parameter in _split_top_level(parameters):
        match = re.match(r"\s*([A-Za-z_][A-Za-z0-9_]*)\s*:", parameter)
        if match is None:
            raise ValueError(f"unsupported shared-expression parameter: {name}/{parameter}")
        names.append(match.group(1))
    return tuple(names)


def _integer_value(expression: str) -> int | None:
    compact = re.sub(r"\s+", "", expression)
    while compact.startswith("(") and compact.endswith(")"):
        compact = compact[1:-1]
    maximum = re.fullmatch(r"u(8|16|32|64|128|256)::MAX", compact)
    if maximum is not None:
        return (1 << int(maximum.group(1))) - 1
    literal = re.fullmatch(r"(-?)([0-9][0-9_]*)(?:(?:u|i)(?:8|16|32|64|128|256))?", compact)
    if literal is None:
        return None
    value = int(literal.group(2).replace("_", ""))
    return -value if literal.group(1) else value


def _constant_aliases(body: str) -> dict[str, int]:
    aliases: dict[str, int] = {}
    for match in re.finditer(
        r"\blet\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s*:\s*[^=;]+)?\s*=\s*([^;]+);",
        body,
    ):
        value = _integer_value(match.group(2))
        if value is not None:
            aliases[match.group(1)] = value
    return aliases


def _normalized_argument(
    argument: str, parameters: tuple[str, ...], aliases: dict[str, int]
) -> tuple[str, int | str]:
    compact = re.sub(r"\s+", "", argument)
    if compact in parameters:
        return ("parameter", parameters.index(compact))
    value = aliases.get(compact)
    if value is None:
        value = _integer_value(compact)
    if value is not None:
        return ("constant", value)
    return ("derived", compact)


def _require_single_macro_arm(cleaned: str, macro: str) -> None:
    markers = list(re.finditer(rf"\bmacro_rules!\s*{re.escape(macro)}\s*\{{", cleaned))
    if not markers:
        raise ValueError(f"missing shared-expression macro definition: {macro}")
    if len(markers) != 1:
        raise ValueError(f"ambiguous shared-expression macro definition: {macro}")
    marker = markers[0]
    body, _ = _balanced(cleaned, marker.end() - 1, "{", "}")
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    arms = 0
    index = 0
    while index < len(body):
        char = body[index]
        if char in "([{":
            stack.append(char)
        elif char in pairs:
            if not stack or stack.pop() != pairs[char]:
                raise ValueError(f"unbalanced shared-expression macro: {macro}")
        elif body.startswith("=>", index) and not stack:
            arms += 1
            index += 1
        index += 1
    if stack or arms != 1:
        raise ValueError(f"shared-expression macro must have exactly one arm: {macro}")


def validate_shared_expression(
    source: str,
    kernel: str,
    macro: str,
    derived_bindings: tuple[tuple[int, int, str], ...] = (),
) -> None:
    cleaned = strip_comments_and_strings(source)
    _require_single_macro_arm(cleaned, macro)
    production = rust_body(cleaned, kernel)
    specification = verus_spec_body(cleaned, f"{kernel}_spec")
    production_calls = _macro_invocations(production, macro)
    specification_calls = _macro_invocations(specification, macro)
    if len(production_calls) != 1 or len(specification_calls) != 1:
        raise ValueError(
            f"shared-expression must call {macro} exactly once in Cargo and Verus: {kernel}"
        )
    production_arguments = production_calls[0]
    specification_arguments = specification_calls[0]
    if len(production_arguments) != len(specification_arguments):
        raise ValueError(f"shared-expression argument count differs: {kernel}/{macro}")

    production_parameters = _function_parameters(cleaned, kernel, specification=False)
    specification_parameters = _function_parameters(
        cleaned, f"{kernel}_spec", specification=True
    )
    specification_aliases = _constant_aliases(specification)
    declared_derived = {
        production_index: (specification_index, expression)
        for production_index, specification_index, expression in derived_bindings
    }
    used_derived: set[int] = set()
    for index, (production_argument, specification_argument) in enumerate(
        zip(production_arguments, specification_arguments, strict=True)
    ):
        production_value = _normalized_argument(
            production_argument, production_parameters, {}
        )
        specification_value = _normalized_argument(
            specification_argument, specification_parameters, specification_aliases
        )
        if production_value == specification_value:
            continue
        if production_value[0] == "derived" and specification_value[0] == "parameter":
            declared = declared_derived.get(index)
            expected = (specification_value[1], re.sub(r"\s+", "", production_argument))
            if declared == expected:
                used_derived.add(index)
                continue
        raise ValueError(
            f"shared-expression argument binding differs: "
            f"{kernel}/{macro}/{index}/{production_value}/{specification_value}"
        )
    unused_derived = set(declared_derived) - used_derived
    if unused_derived:
        raise ValueError(
            f"unused shared-expression derived bindings: {kernel}/{sorted(unused_derived)}"
        )


def main() -> int:
    obligations = parse_verus_manifest(MANIFEST.read_text(encoding="utf-8"))
    kernel_source = KERNEL.read_text(encoding="utf-8")
    cleaned_kernel = strip_comments_and_strings(kernel_source)
    pass_source = PASS.read_text(encoding="utf-8")
    cleaned_pass = strip_comments_and_strings(pass_source)
    kernels = {obligation.kernel for obligation in obligations.values()}
    cleaned_sources: dict[Path, str] = {KERNEL.resolve(): cleaned_kernel}

    for obligation in obligations.values():
        fixture = FAIL / obligation.fixture
        if not fixture.is_file():
            raise ValueError(f"missing Verus negative fixture: {obligation.fixture}")
        validate_proof_binding(
            cleaned_pass,
            obligation.kind,
            obligation.kernel,
            obligation.proof,
        )
        if obligation.kind != "executable" and re.search(
            rf"\bpub\s+open\s+spec\s+fn\s+{re.escape(obligation.kernel)}_spec\s*\(",
            kernel_source,
        ) is None:
            raise ValueError(f"missing Verus spec: {obligation.kernel}")
        if obligation.kind == "shared-expression":
            macro = obligation.binding[0]
            validate_shared_expression(
                kernel_source,
                obligation.kernel,
                macro,
                obligation.derived_bindings,
            )
        if obligation.kind == "derived":
            unknown = set(obligation.binding) - kernels
            if unknown:
                raise ValueError(
                    f"derived Verus obligation has unknown dependencies: "
                    f"{obligation.obligation_id}/{sorted(unknown)}"
                )
        for call_site in obligation.call_sites:
            if call_site.count("#") != 1:
                raise ValueError(f"invalid production call-site: {call_site}")
            path_text, function = call_site.split("#")
            path = production_call_site_path(path_text)
            if path not in cleaned_sources:
                cleaned_sources[path] = strip_comments_and_strings(
                    path.read_text(encoding="utf-8")
                )
            cleaned = cleaned_sources[path]
            body = rust_body(cleaned, function)
            imported_kernel = re.escape(obligation.kernel)
            imported = re.search(
                rf"\buse\s+bridge_core\s*::\s*\{{[^}}]*\b{imported_kernel}\b[^}}]*\}}\s*;",
                cleaned,
                re.DOTALL,
            ) is not None
            unqualified = re.search(
                rf"(?<![A-Za-z0-9_:]){re.escape(obligation.kernel)}\s*\(", body
            ) is not None and re.search(
                rf"\b(?:let|fn)\s+{re.escape(obligation.kernel)}\b", body
            ) is None
            if not production_body_calls(
                body, obligation.kernel, kernel_internal=path == KERNEL.resolve()
            ) and not (imported and unqualified):
                raise ValueError(
                    f"production call-site does not call registered kernel: "
                    f"{call_site} -> {obligation.kernel}"
                )

    registered_specs = {
        f"{obligation.kernel}_spec"
        for obligation in obligations.values()
        if obligation.kind != "executable"
    }
    actual_specs = set(
        re.findall(r"\bpub\s+open\s+spec\s+fn\s+([A-Za-z0-9_]+)\s*\(", kernel_source)
    )
    if registered_specs != actual_specs:
        raise ValueError(
            "Verus spec coverage differs: "
            f"missing={sorted(actual_specs - registered_specs)} "
            f"extra={sorted(registered_specs - actual_specs)}"
        )
    actual_fixtures = {path.name for path in FAIL.glob("*.rs")}
    registered_fixtures = {obligation.fixture for obligation in obligations.values()}
    if registered_fixtures != actual_fixtures:
        raise ValueError(
            "Verus fixture coverage differs: "
            f"missing={sorted(actual_fixtures - registered_fixtures)} "
            f"extra={sorted(registered_fixtures - actual_fixtures)}"
        )
    print(f"Verus manifest passed ({len(obligations)} obligations)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
