#!/usr/bin/env python3
"""Shared lexical analysis for canonical production-to-kernel Rust calls."""

from __future__ import annotations

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
KERNEL = ROOT / "canister" / "bridge-core" / "src" / "kernel.rs"
BRIDGE_CORE_ROOT = (ROOT / "canister" / "bridge-core" / "src").resolve()
BRIDGE_CANISTER_ROOT = (ROOT / "canister" / "bridge-canister" / "src").resolve()


def rust_body(cleaned: str, name: str) -> str:
    marker_pattern = re.compile(
        rf"\b(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+"
        rf"{re.escape(name)}\b(?=\s*(?:<|\())"
    )
    matches = list(marker_pattern.finditer(cleaned))
    if len(matches) != 1:
        raise ValueError(
            f"production call-site function must resolve exactly once: {name}/{len(matches)}"
        )
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{", ">": "<"}
    brace = None
    for index in range(matches[0].end(), len(cleaned)):
        character = cleaned[index]
        if character in "([<":
            stack.append(character)
        elif character == "{" and stack:
            stack.append(character)
        elif character in pairs and stack and stack[-1] == pairs[character]:
            stack.pop()
        elif character == "{" and not stack:
            brace = index
            break
    if brace is None:
        raise ValueError(f"production call-site function has no body: {name}")
    depth = 0
    for index in range(brace, len(cleaned)):
        if cleaned[index] == "{":
            depth += 1
        elif cleaned[index] == "}":
            depth -= 1
            if depth == 0:
                return cleaned[brace : index + 1]
    raise ValueError(f"unbalanced production call-site body: {name}")


def balanced(source: str, start: int, opening: str, closing: str) -> tuple[str, int]:
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


def split_top_level(source: str) -> tuple[str, ...]:
    items: list[str] = []
    start = 0
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    for index, character in enumerate(source):
        if character in "([{":
            stack.append(character)
        elif character in pairs:
            if not stack or stack.pop() != pairs[character]:
                raise ValueError("unbalanced Rust argument")
        elif character == "," and not stack:
            items.append(source[start:index].strip())
            start = index + 1
    if stack:
        raise ValueError("unbalanced Rust argument")
    final = source[start:].strip()
    if final:
        items.append(final)
    return tuple(items)


def rust_function_parameter_names(cleaned: str, name: str) -> frozenset[str]:
    marker = re.compile(
        rf"\b(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?fn\s+"
        rf"{re.escape(name)}\b(?=\s*(?:<|\())"
    )
    matches = list(marker.finditer(cleaned))
    if len(matches) != 1:
        raise ValueError(
            f"production call-site function must resolve exactly once: {name}/{len(matches)}"
        )
    angle_depth = 0
    parameter_start = None
    for index in range(matches[0].end(), len(cleaned)):
        character = cleaned[index]
        if character == "<":
            angle_depth += 1
        elif character == ">" and angle_depth:
            angle_depth -= 1
        elif character == "(" and angle_depth == 0:
            parameter_start = index
            break
    if parameter_start is None:
        raise ValueError(f"production call-site function has no parameters: {name}")
    parameters, _ = balanced(cleaned, parameter_start, "(", ")")
    names: set[str] = set()
    for parameter in split_top_level(parameters):
        match = re.match(
            r"\s*(?:(?:&\s*(?:'[_A-Za-z][_A-Za-z0-9]*\s*)?)?mut\s+|ref\s+)?"
            r"([A-Za-z_][A-Za-z0-9_]*)\s*:",
            parameter,
        )
        if match is not None:
            names.add(match.group(1))
    return frozenset(names)


def _skip_whitespace(source: str, offset: int) -> int:
    while offset < len(source) and source[offset].isspace():
        offset += 1
    return offset


def _turbofish_end(source: str, offset: int) -> int | None:
    offset = _skip_whitespace(source, offset)
    if not source.startswith("::", offset):
        return offset
    offset = _skip_whitespace(source, offset + 2)
    if offset >= len(source) or source[offset] != "<":
        return None
    angle_depth = 0
    delimiters: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    for index in range(offset, len(source)):
        character = source[index]
        if character in "([{":
            delimiters.append(character)
        elif character in pairs:
            if not delimiters or delimiters.pop() != pairs[character]:
                return None
        elif not delimiters and character == "<":
            angle_depth += 1
        elif not delimiters and character == ">":
            angle_depth -= 1
            if angle_depth == 0:
                return index + 1
            if angle_depth < 0:
                return None
    return None


def _rust_call_paths(source: str, pattern: re.Pattern[str]) -> set[str]:
    calls: set[str] = set()
    for match in pattern.finditer(source):
        offset = _turbofish_end(source, match.end())
        if offset is None:
            continue
        offset = _skip_whitespace(source, offset)
        if offset < len(source) and source[offset] == "(":
            calls.add(re.sub(r"\s+", "", match.group("path")))
    return calls


def production_call_is_canonical(
    body: str,
    kernel: str,
    path: Path,
    *,
    source_scope: str | None = None,
    parameter_names: frozenset[str] = frozenset(),
) -> bool:
    escaped = re.escape(kernel)
    resolved = path.resolve()
    source_scope = body if source_scope is None else source_scope
    function_item_alias = re.search(
        rf"\blet\s+(?:mut\s+)?[A-Za-z_][A-Za-z0-9_]*"
        rf"(?:\s*:\s*[^=;]+)?\s*=\s*(?:::)?"
        rf"(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*{escaped}"
        rf"(?:\s*::\s*<[^;]+>)?\s*;",
        body,
    )
    alias_or_shadow = (
        re.search(
            rf"\buse\b[^;]*\b{escaped}\b(?:\s+as\s+[A-Za-z_][A-Za-z0-9_]*)?\s*;",
            source_scope,
        )
        or re.search(rf"\buse\b[^;]*\b{escaped}_body\b[^;]*;", source_scope)
        or re.search(rf"\blet\s+(?:mut\s+)?{escaped}\b", body)
        or re.search(rf"\b(?:fn|struct|enum|type|const|static)\s+{escaped}\b", body)
        or function_item_alias
        or re.search(rf"\bmacro_rules\s*!\s*{escaped}_body\b", body)
    )
    if alias_or_shadow or kernel in parameter_names:
        return False
    symbol_pattern = re.compile(
        rf"(?<![A-Za-z0-9_])(?P<path>(?:::)?"
        rf"(?:(?!(?:break|else|for|if|let|match|return|while)\b)"
        rf"[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*{escaped})\b"
    )
    macro_pattern = re.compile(
        rf"(?<![A-Za-z0-9_])(?P<path>(?:::)?"
        rf"(?:(?!(?:break|else|for|if|let|match|return|while)\b)"
        rf"[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*{escaped}_body)\s*!\s*[({{\[]"
    )
    calls = _rust_call_paths(body, symbol_pattern)
    macros = {
        re.sub(r"\s+", "", match.group("path"))
        for match in macro_pattern.finditer(body)
    }
    if resolved == KERNEL.resolve():
        return bool(calls or macros) and calls <= {f"self::{kernel}"} and macros <= {
            f"{kernel}_body"
        }
    if resolved.is_relative_to(BRIDGE_CORE_ROOT):
        expected = f"crate::kernel::{kernel}"
    elif resolved.is_relative_to(BRIDGE_CANISTER_ROOT):
        expected = f"::bridge_core::kernel::{kernel}"
    else:
        return False
    return bool(calls) and calls == {expected} and not macros
