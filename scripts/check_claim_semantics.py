#!/usr/bin/env python3
"""Reviewable Lean statements and specification links; not semantic equivalence proof."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

from claim_manifest import parse_claim_manifest, parse_conditional_liveness_manifest
from source_resolution import source_path

ROOT = Path(__file__).resolve().parents[1]
SEMANTICS = "verification/claim-semantics.tsv"
DEFINITIONS = "verification/definition-semantics.tsv"
SNAPSHOT = "verification/generated/claim-statements.json"
MARKDOWN = "verification/generated/claim-statements.md"
FIELDS = ("kind", "id", "specification", "definitions", "premises", "conclusion",
          "boundary", "evidence", "external", "review_note")
DEFINITION_FIELDS = ("name", "role", "specification", "meaning")
NAME = re.compile(r"BridgeSpec(?:\.[A-Za-z_][A-Za-z0-9_]*)+")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def read_table(path: Path, fields: tuple[str, ...]) -> list[dict[str, str]]:
    lines = path.read_text().splitlines()
    if not lines or lines[0].split("\t") != list(fields):
        raise ValueError(f"invalid semantics header: {path}")
    result = []
    for number, line in enumerate(lines[1:], 2):
        values = line.split("\t")
        if len(values) != len(fields) or not all(value.strip() for value in values):
            raise ValueError(f"invalid semantics row: {path}:{number}")
        result.append(dict(zip(fields, values, strict=True)))
    return result


def require_source(link: str, root: Path) -> None:
    # Documentation references are paths; production symbols stay in existing typed ledgers.
    path = (root / link).resolve()
    if not path.is_relative_to(root.resolve()) or not path.is_file():
        raise ValueError(f"missing specification: {link}")


def load_registry(root: Path = ROOT) -> tuple[list[dict], list[dict], dict]:
    claims = parse_claim_manifest((root / "verification/claims.tsv").read_text())
    liveness = parse_conditional_liveness_manifest(
        (root / "verification/conditional-liveness.tsv").read_text())
    rows = read_table(root / SEMANTICS, FIELDS)
    definitions = read_table(root / DEFINITIONS, DEFINITION_FIELDS)
    names = {row["name"] for row in definitions}
    if len(names) != len(definitions) or not all(NAME.fullmatch(name) for name in names):
        raise ValueError("duplicate or invalid major definition")
    for row in definitions:
        if row["role"] not in {"specification", "model-support"}:
            raise ValueError(f"invalid definition role: {row['name']}")
        require_source(row["specification"], root)
    expected = {("claim", key) for key in claims.contracts} | {
        ("liveness", key) for key in liveness}
    actual = [(row["kind"], row["id"]) for row in rows]
    if len(set(actual)) != len(actual) or set(actual) != expected:
        raise ValueError("semantics must cover exactly the claim and liveness catalogs")
    roots = {}
    referenced = set()
    for row in rows:
        key = row["id"]
        require_source(row["specification"], root)
        selected = row["definitions"].split(";")
        if len(set(selected)) != len(selected) or not set(selected) <= names:
            raise ValueError(f"unregistered or duplicate definition: {key}")
        referenced.update(selected)
        if row["kind"] == "claim":
            contract = claims.contracts[key]
            roots[key] = {"contract": contract.contract, "witness": contract.witness}
            if row["evidence"] != f"claims.tsv:{key}" or row["external"] != f"claims.tsv:{key}":
                raise ValueError(f"claim evidence must reference existing ledger: {key}")
        else:
            prop = liveness[key]
            roots[key] = {"contract": prop.proposition, "witness": prop.theorem}
            if row["evidence"] != f"conditional-liveness.tsv:{key}" or row["external"] != f"conditional-liveness.tsv:{key}":
                raise ValueError(f"liveness evidence must reference existing ledger: {key}")
        if roots[key]["contract"] not in selected:
            raise ValueError(f"statement definition is not registered: {key}")
    if referenced != names:
        raise ValueError(f"unused major definitions: {sorted(names - referenced)}")
    return sorted(rows, key=lambda r: (r["kind"], r["id"])), sorted(definitions, key=lambda r: r["name"]), roots


def export_source(names: list[str]) -> str:
    if not all(NAME.fullmatch(name) for name in names):
        raise ValueError("invalid Lean declaration name")
    commands = ["import BridgeSpec.AuditExport", "import BridgeSpec.ClaimContracts",
                "import BridgeSpec.Liveness", "import BridgeSpec.ModelBoundaries", "",
                "set_option pp.fullNames true", "set_option pp.universes true",
                "set_option format.width 100", "set_option pp.proofs false", ""]
    for name in names:
        commands += ["run_cmd Lean.Elab.Command.liftTermElabM do",
                     f"  let value ← BridgeSpec.AuditExport.declarationJson `{name}",
                     '  IO.println ("BRIDGE_SEMANTICS " ++ value.compress)']
    return "\n".join(commands) + "\n"


def export_declarations(names: list[str], root: Path) -> list[dict]:
    cwd = root / "verification/lean"
    subprocess.run(["lake", "build", "BridgeSpec.AuditExport", "BridgeSpec.ClaimContracts",
                    "BridgeSpec.Liveness", "BridgeSpec.ModelBoundaries"], cwd=cwd,
                   check=True, capture_output=True, text=True)
    with tempfile.NamedTemporaryFile(mode="w", suffix=".lean") as source:
        source.write(export_source(names))
        source.flush()
        result = subprocess.run(["lake", "env", "lean", source.name], cwd=cwd,
                                check=False, capture_output=True, text=True)
        if result.returncode:
            raise ValueError(f"Lean semantics export failed:\n{result.stdout}{result.stderr}")
    declarations = [json.loads(line.removeprefix("BRIDGE_SEMANTICS "))
                    for line in result.stdout.splitlines() if line.startswith("BRIDGE_SEMANTICS ")]
    if [row["name"] for row in declarations] != names:
        raise ValueError("Lean did not export every requested declaration exactly once")
    return declarations


def source_inventory(root: Path) -> dict:
    return {path.relative_to(root).as_posix(): {
        "sha256": digest(path.read_bytes()),
        "lines": len(path.read_text().splitlines()),
        "source_declarations": len(re.findall(
            r"^(?:noncomputable |private )?(?:def|abbrev|theorem|lemma|structure|inductive|instance)\b",
            path.read_text(), re.MULTILINE)),
    } for path in sorted((root / "verification/lean").rglob("*.lean"))
        if ".lake" not in path.parts}


def normalized_lean_version(output: str) -> str:
    match = re.fullmatch(r"Lean \(version ([^,]+), [^,]+, commit ([0-9a-f]+), [^)]+\)", output.strip())
    if match is None:
        raise ValueError("unrecognized Lean version output")
    return f"Lean {match[1]} commit {match[2]}"


def build_snapshot(root: Path = ROOT) -> dict:
    rows, definitions, roots = load_registry(root)
    names = sorted({row["name"] for row in definitions} |
                   {value["witness"] for value in roots.values()})
    before = source_inventory(root)
    declarations = export_declarations(names, root)
    after = source_inventory(root)
    if before != after:
        raise ValueError("Lean source changed during semantics export")
    version = subprocess.check_output(["lake", "env", "lean", "--version"],
                                     cwd=root / "verification/lean", text=True).strip()
    return {"schema": 1, "lean_version": normalized_lean_version(version),
            "toolchain": (root / "lean-toolchain").read_text().strip(),
            "exporter_sha256": digest(source_path("scripts/check_claim_semantics.py", root).read_bytes()),
            "sources": after, "semantics": rows, "definitions": definitions,
            "roots": roots, "declarations": declarations}


def render_markdown(snapshot: dict) -> str:
    declarations = {row["name"]: row for row in snapshot["declarations"]}
    output = ["# Claim statements and specification correspondence", "",
              "Proposition types and major definitions interpreted by Lean. Proof bodies are omitted. This is not evidence of semantic agreement with the specification or independent kernel checking.", "",
              f"Toolchain: `{snapshot['toolchain']}`", ""]
    for row in snapshot["semantics"]:
        root = snapshot["roots"][row["id"]]
        output += [f"## {row['kind']}: {row['id']}", "",
                   f"Specification: `{row['specification']}`", "",
                   f"Premises: {row['premises']}", "", f"Conclusion: {row['conclusion']}", "",
                   f"Unproved boundary: {row['boundary']}", "",
                   f"Evidence and external assumptions: `{row['evidence']}` / `{row['external']}`", "",
                   f"Review rationale: {row['review_note']}", "", "```lean",
                   root["witness"] + " : " + declarations[root["witness"]]["type"], "```", "",
                   "Major definitions: " + ", ".join(f"`{name}`" for name in row["definitions"].split(";")), ""]
    output += ["## Shared definitions", ""]
    for row in snapshot["definitions"]:
        declaration = declarations[row["name"]]
        output += [f"### {row['name']}", "", f"{row['role']}: {row['meaning']}", "",
                   f"Specification: `{row['specification']}`", "", "```lean",
                   row["name"] + " : " + declaration["type"],
                   declaration["definition"] or "-- Structure or inductive type. Source digests also report field and other changes.",
                   "```", ""]
    output += ["## Source inventory (conservative change detection)", "",
               "| Source | Lines | Declarations | SHA-256 |", "|---|---:|---:|---|"]
    for path, info in snapshot["sources"].items():
        output += [f"| {path} | {info['lines']} | {info['source_declarations']} | {info['sha256']} |"]
    return "\n".join(output) + "\n"


def compare_snapshots(base: dict, current: dict) -> dict:
    before = {r["name"]: r for r in base["declarations"]}
    after = {r["name"]: r for r in current["declarations"]}
    changed = sorted(name for name in before.keys() | after.keys() if before.get(name) != after.get(name))
    source_changes = sorted(path for path in base["sources"].keys() | current["sources"].keys()
                            if base["sources"].get(path) != current["sources"].get(path))
    old_rows = {r["id"]: r for r in base["semantics"]}
    missing_notes = []
    for row in current["semantics"]:
        names = set(row["definitions"].split(";")) | set(current["roots"][row["id"]].values())
        if names.intersection(changed) and old_rows.get(row["id"]) == row:
            missing_notes.append(row["id"])
    return {"changed_declarations": changed, "changed_sources": source_changes,
            "missing_semantics_updates": missing_notes,
            "review_required": bool(changed or source_changes or base != current),
            "interpretation": "Changes require human review; this is not a weakening/equivalence decision."}


def check_semantics(root: Path = ROOT, write: bool = False) -> dict:
    snapshot = build_snapshot(root)
    outputs = {SNAPSHOT: json.dumps(snapshot, ensure_ascii=False, indent=2) + "\n",
               MARKDOWN: render_markdown(snapshot)}
    for relative, expected in outputs.items():
        path = root / relative
        if write:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(expected)
        elif not path.is_file() or path.read_text() != expected:
            raise ValueError(f"stale semantics artifact: {relative}; run check_claim_semantics.py --write and review")
    return snapshot


def check_base(snapshot: dict, base_sha: str, root: Path = ROOT) -> dict:
    if not re.fullmatch(r"[0-9a-f]{40}", base_sha):
        raise ValueError("base must be an immutable full commit SHA")
    subprocess.run(["git", "cat-file", "-e", base_sha + "^{commit}"], cwd=root, check=True)
    entry = subprocess.check_output(
        ["git", "ls-tree", "-z", base_sha, "--", SNAPSHOT], cwd=root)
    if not entry:
        return {"bootstrap": True, "review_required": True,
                "base_sha": base_sha, "reason": "trusted base has no statement snapshot"}
    base = json.loads(subprocess.check_output(
        ["git", "show", f"{base_sha}:{SNAPSHOT}"], cwd=root, text=True))
    comparison = compare_snapshots(base, snapshot)
    if comparison["missing_semantics_updates"]:
        raise ValueError("changed statements/definitions need updated semantics rows: " +
                         ", ".join(comparison["missing_semantics_updates"]))
    return {"base_sha": base_sha, **comparison}


def check_reviewed_semantics(root: Path = ROOT) -> None:
    snapshot = check_semantics(root)
    base = os.environ.get("BRIDGE_TRUSTED_BASE_SHA")
    if base:
        print(json.dumps(check_base(snapshot, base, root), ensure_ascii=False, sort_keys=True))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--base-sha", help="trusted base commit; never a candidate-supplied JSON baseline")
    args = parser.parse_args()
    snapshot = check_semantics(write=args.write)
    base = args.base_sha or os.environ.get("BRIDGE_TRUSTED_BASE_SHA")
    if base:
        print(json.dumps(check_base(snapshot, base), ensure_ascii=False, indent=2))
    print("claim semantics passed (43 release claims, 5 conditional liveness properties)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
