#!/usr/bin/env python3
"""Regression tests for the fail-closed Certora advisory manifest."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

import check_certora_manifest
from check_solidity_ast_bindings import AstIndex


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
        self.ast_index = AstIndex(
            self.root / "contracts/out", self.root / "contracts", self.root
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_metadata_mutations_are_rejected(self) -> None:
        for filename, key, value, error in (
            ("Bridge.conf", "url_visibility", "public", "url_visibility"),
            ("BSNS.conf", "optimistic_summary_recursion", True, "under-approximating"),
            ("Bridge.conf", "rule_sanity", "basic", "rule_sanity"),
        ):
            path = self.root / "verification/certora/confs" / filename
            original = path.read_text(encoding="utf-8")
            with self.subTest(config=filename, key=key):
                config = json.loads(original)
                config[key] = value
                try:
                    path.write_text(json.dumps(config), encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, error):
                        check_certora_manifest.validate(self.root, self.ast_index)
                finally:
                    path.write_text(original, encoding="utf-8")

        path = self.root / "verification/certora/obligations.tsv"
        original = path.read_text(encoding="utf-8")
        symbol = "contracts/src/BSNS.sol#BSNS.bridgeMint(address,uint256)"
        mutations = [
            ("unknown claim", original.replace("deposit_admission;deposit_backing;exact_mint_finalization", "not_a_claim", 1), "unknown Certora claims"),
            ("noncanonical symbol", original.replace(symbol, "contracts/src/BSNS.sol#bridgeMint", 1), "canonical Solidity function link"),
            ("wrong signature", original.replace(symbol, "contracts/src/BSNS.sol#BSNS.bridgeMint(address,uint128)", 1), "unresolved Solidity AST function link"),
            ("wrong contract", original.replace(symbol, "contracts/src/BSNS.sol#Decoy.bridgeMint(address,uint256)", 1), "unresolved Solidity AST function link"),
            ("missing symbol", original.replace("BSNS.bridgeMint", "BSNS.definitelyMissing"), "unresolved Solidity AST function link"),
            ("noncanonical link", original.replace("BSNS.bridgeMint", "bridgeMint"), "invalid canonical Solidity function link"),
        ]
        for row, extra, error in (
            (1, None, "duplicate or empty rule entry"),
            (2, "verification/certora/specs/Bridge.spec#mintAppliesExactEffects", "duplicate Certora rule ownership"),
        ):
            lines = original.splitlines()
            fields = lines[row].split("\t")
            fields[3] += ";" + (extra or fields[3].split(";", 1)[0])
            lines[row] = "\t".join(fields)
            mutations.append((error, "\n".join(lines) + "\n", error))
        for name, changed, error in mutations:
            with self.subTest(mutation=name):
                self.assertNotEqual(changed, original, "mutation must change the fixture")
                try:
                    path.write_text(changed, encoding="utf-8")
                    with self.assertRaisesRegex(ValueError, error):
                        check_certora_manifest.validate(self.root, self.ast_index)
                finally:
                    path.write_text(original, encoding="utf-8")

    def test_stale_solidity_ast_is_rejected(self) -> None:
        source = self.root / "contracts/src/BSNS.sol"
        artifact = self.root / "contracts/out/BSNS.sol/BSNS.json"
        source.write_bytes(source.read_bytes() + b"\n")
        artifact_mtime = artifact.stat().st_mtime_ns
        os.utime(source, ns=(artifact_mtime, artifact_mtime))
        with self.assertRaisesRegex(ValueError, "stale Solidity AST source link"):
            check_certora_manifest.validate(self.root)

    def test_unowned_rule_is_rejected(self) -> None:
        path = self.root / "verification/certora/specs/BSNS.spec"
        path.write_text(path.read_text(encoding="utf-8") + "\nrule unownedRule() { assert true; }\n", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "missing obligation ownership"):
            check_certora_manifest.validate(self.root, self.ast_index)

    def test_runner_redacts_secrets_before_console_and_artifact_output(self) -> None:
        source = self.root / "raw-certora.log"
        destination = self.root / "sanitized-certora.log"
        secret = "certora-secret-value"
        source.write_text(
            f"key={secret}\n"
            "https://prover.certora.com/output/?anonymousKey=public-token&jobId=private-token\n",
            encoding="utf-8",
        )
        environment = os.environ.copy()
        environment["CERTORAKEY"] = secret
        result = subprocess.run(
            [
                str(ROOT / "scripts/run_certora_advisory.sh"),
                "--test-redaction",
                str(source),
                str(destination),
            ],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
        )
        persisted = destination.read_text(encoding="utf-8")
        for output in (result.stdout, result.stderr, persisted):
            self.assertNotIn(secret, output)
            self.assertNotIn("public-token", output)
            self.assertNotIn("private-token", output)
        self.assertIn("[REDACTED_CERTORAKEY]", persisted)
        self.assertIn("anonymousKey=[REDACTED]", persisted)
        self.assertIn("jobId=[REDACTED]", persisted)


if __name__ == "__main__":
    unittest.main()
