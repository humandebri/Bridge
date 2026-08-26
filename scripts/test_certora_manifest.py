#!/usr/bin/env python3
"""Regression tests for the fail-closed Certora advisory manifest."""

from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path

import check_certora_manifest


ROOT = Path(__file__).resolve().parents[1]


class CertoraManifestTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        shutil.copytree(ROOT / "verification/certora", self.root / "verification/certora")
        for relative in ("verification/claims.tsv", "verification/assumptions.tsv"):
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, target)
        for source in (ROOT / "contracts/src").rglob("*.sol"):
            target = self.root / source.relative_to(ROOT)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        for contract in ("Bridge.sol", "BSNS.sol", "BridgeTimelockController.sol"):
            shutil.copytree(
                ROOT / "contracts/out" / contract,
                self.root / "contracts/out" / contract,
            )
        openzeppelin = self.root / "contracts/lib/openzeppelin-contracts/contracts"
        openzeppelin.mkdir(parents=True)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def config(self, name: str) -> tuple[Path, dict[str, object]]:
        path = self.root / "verification/certora/confs" / name
        return path, json.loads(path.read_text(encoding="utf-8"))

    def test_current_manifest_is_valid(self) -> None:
        check_certora_manifest.validate(ROOT)

    def test_public_report_is_rejected(self) -> None:
        path, config = self.config("Bridge.conf")
        config["url_visibility"] = "public"
        path.write_text(json.dumps(config), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "url_visibility"):
            check_certora_manifest.validate(self.root)

    def test_optimistic_option_is_rejected(self) -> None:
        path, config = self.config("BSNS.conf")
        config["optimistic_summary_recursion"] = True
        path.write_text(json.dumps(config), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "under-approximating"):
            check_certora_manifest.validate(self.root)

    def test_unknown_claim_is_rejected(self) -> None:
        path = self.root / "verification/certora/obligations.tsv"
        text = path.read_text(encoding="utf-8")
        path.write_text(text.replace("deposit_admission;deposit_backing;exact_mint_finalization", "not_a_claim", 1), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "unknown Certora claims"):
            check_certora_manifest.validate(self.root)

    def test_missing_solidity_symbol_is_rejected(self) -> None:
        path = self.root / "verification/certora/obligations.tsv"
        text = path.read_text(encoding="utf-8")
        path.write_text(text.replace("BSNS.bridgeMint", "BSNS.definitelyMissing"), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "unresolved Solidity AST source link"):
            check_certora_manifest.validate(self.root)

    def test_noncanonical_solidity_link_is_rejected(self) -> None:
        path = self.root / "verification/certora/obligations.tsv"
        text = path.read_text(encoding="utf-8")
        path.write_text(text.replace("BSNS.bridgeMint", "bridgeMint"), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "invalid canonical Solidity source link"):
            check_certora_manifest.validate(self.root)

    def test_stale_solidity_ast_is_rejected(self) -> None:
        source = self.root / "contracts/src/BSNS.sol"
        artifact = self.root / "contracts/out/BSNS.sol/BSNS.json"
        artifact.touch()
        source.touch()
        with self.assertRaisesRegex(ValueError, "stale Solidity AST source link"):
            check_certora_manifest.validate(self.root)

    def test_duplicate_rule_within_row_is_rejected(self) -> None:
        path = self.root / "verification/certora/obligations.tsv"
        lines = path.read_text(encoding="utf-8").splitlines()
        fields = lines[1].split("\t")
        first_rule = fields[3].split(";", 1)[0]
        fields[3] += f";{first_rule}"
        lines[1] = "\t".join(fields)
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "duplicate or empty rule entry"):
            check_certora_manifest.validate(self.root)

    def test_duplicate_rule_ownership_is_rejected(self) -> None:
        path = self.root / "verification/certora/obligations.tsv"
        lines = path.read_text(encoding="utf-8").splitlines()
        fields = lines[2].split("\t")
        fields[3] += ";verification/certora/specs/Bridge.spec#mintAppliesExactEffects"
        lines[2] = "\t".join(fields)
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "duplicate Certora rule ownership"):
            check_certora_manifest.validate(self.root)

    def test_disabled_advanced_sanity_is_rejected(self) -> None:
        path, config = self.config("Bridge.conf")
        config["rule_sanity"] = "basic"
        path.write_text(json.dumps(config), encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "rule_sanity"):
            check_certora_manifest.validate(self.root)

    def test_unowned_rule_is_rejected(self) -> None:
        path = self.root / "verification/certora/specs/BSNS.spec"
        path.write_text(path.read_text(encoding="utf-8") + "\nrule unownedRule() { assert true; }\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "missing obligation ownership"):
            check_certora_manifest.validate(self.root)


if __name__ == "__main__":
    unittest.main()
