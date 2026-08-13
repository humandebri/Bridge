#!/usr/bin/env python3
"""Strict helpers for the small Candid text subset used by Plan 007 drivers."""

from __future__ import annotations

import re
import sys


def _assignments(candid: str, field: str) -> list[int]:
    return [match.end() for match in re.finditer(rf"\b{re.escape(field)}\s*=\s*", candid)]


def _quoted(candid: str, offset: int) -> tuple[str, int]:
    if offset >= len(candid) or candid[offset] != '"':
        raise ValueError("expected quoted Candid value")
    result: list[str] = []
    index = offset + 1
    while index < len(candid):
        character = candid[index]
        if character == '"':
            return "".join(result), index + 1
        if character == "\\":
            if index + 1 >= len(candid):
                raise ValueError("truncated Candid escape")
            result.append(candid[index:index + 2])
            index += 2
            continue
        if ord(character) < 0x20:
            raise ValueError("raw control character in Candid string")
        result.append(character)
        index += 1
    raise ValueError("unterminated Candid string")


def decode_blob_literal(value: str) -> bytes:
    output = bytearray()
    index = 0
    named = {"n": b"\n", "r": b"\r", "t": b"\t", "\\": b"\\", '"': b'"'}
    while index < len(value):
        if value[index] != "\\":
            end = value.find("\\", index)
            if end < 0:
                end = len(value)
            output.extend(value[index:end].encode("utf-8"))
            index = end
            continue
        if index + 1 >= len(value):
            raise ValueError("truncated Candid blob escape")
        escape = value[index + 1]
        if escape in named:
            output.extend(named[escape])
            index += 2
        elif index + 2 < len(value) and re.fullmatch(r"[0-9a-fA-F]{2}", value[index + 1:index + 3]):
            output.append(int(value[index + 1:index + 3], 16))
            index += 3
        else:
            raise ValueError(f"unsupported Candid blob escape: \\{escape}")
    return bytes(output)


def blob(candid: str, field: str, *, length: int | None = None) -> bytes:
    offsets = _assignments(candid, field)
    if len(offsets) != 1:
        raise ValueError(f"Candid must expose exactly one {field}")
    offset = offsets[0]
    blob_match = re.match(r'blob\s*', candid[offset:])
    if blob_match:
        literal, _ = _quoted(candid, offset + blob_match.end())
        value = decode_blob_literal(literal)
    else:
        vector = re.match(r"vec\s*\{([^}]*)\}", candid[offset:], re.S)
        if not vector:
            raise ValueError(f"Candid {field} is not a blob or vec nat8")
        body = vector.group(1)
        tokens = re.findall(r"([0-9][0-9_]*)\s*:\s*nat8\b", body)
        residue = re.sub(r"([0-9][0-9_]*)\s*:\s*nat8\b", "", body)
        if residue.replace(";", "").strip():
            raise ValueError(f"Candid {field} contains a non-nat8 vector element")
        values = [int(token.replace("_", "")) for token in tokens]
        if any(value > 255 for value in values):
            raise ValueError(f"Candid {field} contains an out-of-range nat8")
        value = bytes(values)
    if length is not None and len(value) != length:
        raise ValueError(f"Candid {field} must contain exactly {length} bytes")
    return value


def nat(candid: str, field: str) -> int:
    offsets = _assignments(candid, field)
    if len(offsets) != 1:
        raise ValueError(f"Candid must expose exactly one {field}")
    match = re.match(r"([0-9][0-9_]*)\s*(?::\s*nat(?:8|16|32|64)?)?\b", candid[offsets[0]:])
    if not match:
        raise ValueError(f"Candid {field} is not a natural number")
    return int(match.group(1).replace("_", ""))


def principal(candid: str, field: str) -> str:
    offsets = _assignments(candid, field)
    if len(offsets) != 1:
        raise ValueError(f"Candid must expose exactly one {field}")
    match = re.match(r"principal\s*", candid[offsets[0]:])
    if not match:
        raise ValueError(f"Candid {field} is not a principal")
    value, _ = _quoted(candid, offsets[0] + match.end())
    if not re.fullmatch(r"[a-z0-9-]+", value):
        raise ValueError(f"Candid {field} is not a canonical principal")
    return value


def integrity_ok(candid: str) -> bool:
    return bool(re.search(r"\bOk\s*=\s*\"ok\"", candid)) and not re.search(r"\bErr\s*=", candid)


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] != "blob32":
        raise SystemExit("usage: candid_values.py blob32 <field>")
    print("0x" + blob(sys.stdin.read(), sys.argv[2], length=32).hex())
