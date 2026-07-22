#!/usr/bin/env python3
"""Require every Lean vector section to name its model, theorem, and consumers."""

from __future__ import annotations

import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
VECTORS = ROOT / "verification" / "generated" / "protocol-vectors.json"
MANIFEST = ROOT / "verification" / "refinement-manifest.tsv"
MODEL = ROOT / "verification" / "lean" / "BridgeSpec" / "Model.lean"
THEOREMS = ROOT / "verification" / "lean" / "BridgeSpec" / "Theorems.lean"


def main() -> int:
    document = json.loads(VECTORS.read_text(encoding="utf-8"))
    if document.get("schema_version") != 1:
        raise SystemExit("protocol vector schema must be exactly v1")
    vector_sections = {
        key
        for key, value in document.items()
        if key.endswith("_cases") and isinstance(value, list)
    }
    rows: dict[str, tuple[str, str, list[str]]] = {}
    for number, line in enumerate(MANIFEST.read_text(encoding="utf-8").splitlines(), 1):
        fields = line.split("\t")
        if len(fields) != 4 or not all(fields):
            raise SystemExit(f"invalid refinement manifest row {number}")
        section, definition, theorem, consumer_list = fields
        if section in rows:
            raise SystemExit(f"duplicate refinement section: {section}")
        rows[section] = (definition, theorem, consumer_list.split(","))
    if set(rows) != vector_sections:
        raise SystemExit(
            f"refinement manifest sections {sorted(rows)} do not match vectors {sorted(vector_sections)}"
        )

    model = MODEL.read_text(encoding="utf-8")
    theorems = THEOREMS.read_text(encoding="utf-8")
    for section, (definition, theorem, consumers) in rows.items():
        cases = document[section]
        if not cases:
            raise SystemExit(f"protocol vector section is empty: {section}")
        if f"def {definition}" not in model:
            raise SystemExit(f"Lean model definition is missing: {definition}")
        if f"theorem {theorem}" not in theorems:
            raise SystemExit(f"Lean theorem is missing: {theorem}")
        if not consumers:
            raise SystemExit(f"refinement section has no consumer: {section}")
        for relative in consumers:
            path = ROOT / relative
            if not path.is_file():
                raise SystemExit(f"refinement consumer is missing: {relative}")
            if section not in path.read_text(encoding="utf-8"):
                raise SystemExit(f"refinement consumer does not name {section}: {relative}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
