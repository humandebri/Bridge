#!/usr/bin/env python3
"""Keep every staging consumer bound to the metadata-bearing Wasm artifact."""

from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
CANONICAL = "target/test-deployment/staging/bridge_canister.wasm"
RAW = "target/test-deployment/wasm32-unknown-unknown/release/bridge_canister.wasm"


class StagingWasmArtifactTests(unittest.TestCase):
    def test_recipe_delegates_to_the_canonical_builder(self) -> None:
        recipe = (ROOT / "recipes/test-bridge-rust.hbs").read_text(encoding="utf-8")
        self.assertIn("scripts/plan007/build-staging-canister-wasm.sh", recipe)
        self.assertNotIn(RAW, recipe)
        self.assertNotIn("metadata \"candid:service\"", recipe)

    def test_deployment_consumers_never_use_the_raw_cargo_artifact(self) -> None:
        consumers = (
            "scripts/ci-local.sh",
            "scripts/plan007/generate-local-e2e.mjs",
            "integration/phase3.spec.ts",
            "docs/runbooks/sepolia-staging-e2e.md",
        )
        for relative in consumers:
            with self.subTest(relative=relative):
                source = (ROOT / relative).read_text(encoding="utf-8")
                self.assertIn(CANONICAL, source)
                self.assertNotIn(RAW, source)


if __name__ == "__main__":
    unittest.main()
