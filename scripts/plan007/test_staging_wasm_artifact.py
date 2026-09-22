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
            "scripts/plan007/generate-local-e2e.mjs",
            "integration/phase3.spec.ts",
            "docs/runbooks/sepolia-staging-e2e.md",
        )
        for relative in consumers:
            with self.subTest(relative=relative):
                source = (ROOT / relative).read_text(encoding="utf-8")
                self.assertIn(CANONICAL, source)
                self.assertNotIn(RAW, source)

    def test_full_integration_entrypoint_builds_metadata_wasm_first(self) -> None:
        package = (ROOT / "package.json").read_text(encoding="utf-8")
        builder = package.index("scripts/plan007/build-staging-canister-wasm.sh")
        jest = package.index("jest --config integration/jest.config.js")
        self.assertLess(builder, jest)

    def test_schema35_predecessor_uses_the_canonical_artifact_unchanged(self) -> None:
        helper = (
            ROOT / "scripts/plan007/build-schema35-predecessor-wasm.sh"
        ).read_text(encoding="utf-8")
        phase3 = (ROOT / "integration/phase3.spec.ts").read_text(encoding="utf-8")
        self.assertEqual(helper.count("build-staging-canister-wasm.sh"), 1)
        self.assertNotIn("ic-wasm", helper)
        self.assertIn("CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0", helper)
        self.assertIn(
            '"darwin-arm64": "621e864035988f1f47169576d68251c80fe369ff155aa17ab55d201c07669730"',
            phase3,
        )


if __name__ == "__main__":
    unittest.main()
