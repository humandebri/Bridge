#!/usr/bin/env python3
"""Regression tests for generated refinement registration and exact-one execution."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

import current_main_check_refinement_manifest as refinement
import current_main_generate_refinement_harness as generator
from current_main_generate_refinement_harness import Renderer


class RefinementManifestTests(unittest.TestCase):
    def write_bridge_constructor(self, root: Path, parameters: str) -> None:
        bridge = root / "contracts/src/Bridge.sol"
        bridge.parent.mkdir(parents=True, exist_ok=True)
        bridge.write_text(
            f"contract Bridge {{ constructor({parameters}) EIP712(\"KINIC Bridge\", \"1\") {{}} }}\n",
            encoding="utf-8",
        )

    def test_generator_supports_legacy_and_current_bridge_constructors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_bridge_constructor(
                root,
                "string memory tokenName, string memory tokenSymbol, uint8 tokenDecimals, "
                "address initialBridgeSigner, address initialRuntimeAdministrator, "
                "address initialBaseAdminTimelock, bytes32 initialApprovedTimelockRuntimeCodeHash, "
                "uint256 initialPerDepositLimit, uint256 initialMintWindowLimit, "
                "uint64 initialMintWindowDuration, uint256 maxServiceFee, uint256 initialServiceFee",
            )
            self.assertEqual(generator.bridge_constructor_prefix(root), '"kinic", "KINIC", 8, ')

            self.write_bridge_constructor(
                root,
                "address initialBridgeSigner, address initialRuntimeAdministrator, "
                "address initialBaseAdminTimelock, bytes32 initialApprovedTimelockRuntimeCodeHash, "
                "uint256 initialPerDepositLimit, uint256 initialMintWindowLimit, "
                "uint64 initialMintWindowDuration, uint256 maxServiceFee, uint256 initialServiceFee",
            )
            self.assertEqual(generator.bridge_constructor_prefix(root), "")

    def test_generator_rejects_unknown_bridge_constructor(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_bridge_constructor(root, "address unexpectedAdministrator")
            with self.assertRaisesRegex(ValueError, "unsupported Bridge constructor"):
                generator.bridge_constructor_prefix(root)

    def fixture(self) -> tuple[Path, dict[str, object], str, str, str, str, dict[tuple[str, str], Renderer]]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        target = root / "generated/example.rs"
        target.parent.mkdir(parents=True)
        target.write_text("// generated harness target\n", encoding="utf-8")
        document = {"schema_version": 3, "example_cases": [{"accepted": True}]}
        manifest = "example_cases\texample\texampleImpl\texample_refinement\trust"
        model = "def example : Bool := true\n"
        implementation = "def exampleImpl : Bool := true\n"
        theorem = "theorem example_refinement : exampleImpl = example := by\n  rfl\n"
        renderers = {
            ("example_cases", "rust"): Renderer(
                "generated/example.rs",
                "protocol_example_cases_matches_production",
                "#[test]\nfn protocol_example_cases_matches_production() {}\n",
            )
        }
        return root, document, manifest, model, implementation, theorem, renderers

    def parse(self, fixture: tuple[Path, dict[str, object], str, str, str, str, dict[tuple[str, str], Renderer]]):
        root, document, manifest, model, implementation, theorem, renderers = fixture
        return refinement.parse_manifest(
            document, manifest, model, implementation, theorem, root, renderers
        )

    def test_manifest_requires_exact_vector_section_coverage(self) -> None:
        fixture = list(self.fixture())
        fixture[1]["missing_cases"] = [{"accepted": False}]
        with self.assertRaisesRegex(ValueError, "do not match vectors"):
            self.parse(tuple(fixture))

    def test_missing_renderer_is_rejected(self) -> None:
        fixture = list(self.fixture())
        fixture[-1] = {}
        with self.assertRaisesRegex(ValueError, "missing refinement renderer"):
            self.parse(tuple(fixture))

    def test_unregistered_renderer_is_rejected(self) -> None:
        fixture = list(self.fixture())
        fixture[-1][("extra_cases", "rust")] = Renderer(
            "generated/example.rs", "extra", "#[test]\nfn extra() {}\n"
        )
        with self.assertRaisesRegex(ValueError, "renderer coverage differs"):
            self.parse(tuple(fixture))

    def test_duplicate_runner_is_rejected(self) -> None:
        fixture = list(self.fixture())
        fixture[2] += "\n" + fixture[2]
        with self.assertRaisesRegex(ValueError, "duplicate refinement consumer"):
            self.parse(tuple(fixture))

    def test_theorem_names_in_hypotheses_do_not_establish_refinement(self) -> None:
        fixture = list(self.fixture())
        fixture[5] = "theorem example_refinement (h : exampleImpl = example) : True := by\n  trivial\n"
        with self.assertRaisesRegex(ValueError, "top-level equality"):
            self.parse(tuple(fixture))

    def test_unrelated_conjunction_does_not_establish_refinement(self) -> None:
        fixture = list(self.fixture())
        fixture[5] = "theorem example_refinement : exampleImpl = true ∧ example = true := by\n  simp\n"
        with self.assertRaisesRegex(ValueError, "top-level equality"):
            self.parse(tuple(fixture))

    def test_refinement_sides_cannot_be_reversed(self) -> None:
        fixture = list(self.fixture())
        fixture[5] = "theorem example_refinement : example = exampleImpl := by\n  rfl\n"
        with self.assertRaisesRegex(ValueError, "must place"):
            self.parse(tuple(fixture))

    def test_manifest_cannot_register_target_selector_or_symbol_strings(self) -> None:
        fixture = list(self.fixture())
        fixture[2] += "\tgenerated/fake.rs\tfake_selector\tfake_symbol"
        with self.assertRaisesRegex(ValueError, "invalid refinement manifest row"):
            self.parse(tuple(fixture))

    def test_rust_consumer_requires_one_passing_test(self) -> None:
        consumer = refinement.Consumer(
            "example_cases", "example", "exampleImpl", "example_refinement", "rust",
            "generated/example.rs", "protocol_example_cases_matches_production",
        )

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            return subprocess.CompletedProcess([], 0, "running 0 tests\n", "")

        with self.assertRaisesRegex(ValueError, "did not pass exactly once"):
            refinement.execute_consumer(consumer, Path("."), runner)

    def test_json_consumer_retries_one_empty_success(self) -> None:
        consumer = refinement.Consumer(
            "example_cases", "example", "exampleImpl", "example_refinement", "vitest",
            "ui/generated/example.test.ts", "protocol_example_cases_matches_production",
        )
        calls = 0

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            nonlocal calls
            calls += 1
            if calls == 1:
                return subprocess.CompletedProcess([], 0, "", "")
            return subprocess.CompletedProcess(
                [],
                0,
                '{"numPassedTests":1,"testResults":[{"assertionResults":['
                '{"title":"protocol_example_cases_matches_production",'
                '"status":"passed"}]}]}',
                "",
            )

        refinement.execute_consumer(consumer, Path("."), runner)
        self.assertEqual(calls, 2)

    def test_vitest_consumer_uses_direct_binary_and_rejects_stdout_noise(self) -> None:
        consumer = refinement.Consumer(
            "example_cases", "example", "exampleImpl", "example_refinement", "vitest",
            "ui/generated/example.test.ts", "protocol_example_cases_matches_production",
        )
        commands: list[object] = []

        def runner(command: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            commands.append(command)
            return subprocess.CompletedProcess([], 0, "pnpm warning\n{}", "")

        with self.assertRaisesRegex(ValueError, "non-JSON stdout"):
            refinement.execute_consumer(consumer, Path("."), runner)
        self.assertTrue(str(commands[0][0]).endswith("ui/node_modules/.bin/vitest"))
        self.assertNotIn("pnpm", commands[0])

    def test_json_consumer_rejects_repeated_empty_success(self) -> None:
        consumer = refinement.Consumer(
            "example_cases", "example", "exampleImpl", "example_refinement", "vitest",
            "ui/generated/example.test.ts", "protocol_example_cases_matches_production",
        )

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            return subprocess.CompletedProcess([], 0, "", "")

        with self.assertRaisesRegex(ValueError, "produced no JSON"):
            refinement.execute_consumer(consumer, Path("."), runner)

    def test_generator_emits_tracked_language_test_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outputs = generator.generated_sources(root)

            self.assertIn(root / generator.RUST_OUTPUT.relative_to(generator.ROOT), outputs)
            self.assertIn(root / generator.FOUNDRY_OUTPUT.relative_to(generator.ROOT), outputs)
            self.assertIn(root / generator.VITEST_OUTPUT.relative_to(generator.ROOT), outputs)
            self.assertIn("committed_quote_matches", "".join(outputs.values()))
            self.assertIn("MintAuthorizationPolicy.evaluateMint", "".join(outputs.values()))
            self.assertIn("decideWithdrawalFinalization", "".join(outputs.values()))
            self.assertIn("contract GeneratedRefinementTest", "".join(outputs.values()))

    def test_handwritten_tests_are_not_generator_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            canonical = root / "canister/bridge-core/tests/protocol_vectors.rs"
            canonical.parent.mkdir(parents=True)
            canonical.write_text("#[test]\nfn fake() { assert!(true); }\n", encoding="utf-8")
            before = generator.generated_sources(root)
            canonical.write_text("#[test]\nfn changed() { panic!(); }\n", encoding="utf-8")
            self.assertEqual(generator.generated_sources(root), before)

    def test_generated_source_modification_is_stale(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outputs = generator.expected_outputs(root)
            for path, source in outputs.items():
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(source, encoding="utf-8")

            rust_output = root / generator.RUST_OUTPUT.relative_to(generator.ROOT)
            rust_output.write_text(
                rust_output.read_text(encoding="utf-8").replace(
                    "assert_eq!(actual, boolean(&case, \"accepted\"));",
                    "assert!(true);",
                    1,
                ),
                encoding="utf-8",
            )

            self.assertEqual(generator.stale_outputs(root), [rust_output])

    def test_each_registered_renderer_owns_one_generated_selector(self) -> None:
        outputs = generator.generated_sources()
        for (section, _runner), renderer in generator.RENDERERS.items():
            generated = outputs[generator.ROOT / renderer.target]
            self.assertEqual(generated.count(renderer.selector), 1)
            self.assertIn(section, generated)

    def test_non_generated_selector_owner_is_rejected(self) -> None:
        root, document, manifest, model, implementation, theorem, renderers = self.fixture()
        consumers = refinement.parse_manifest(
            document, manifest, model, implementation, theorem, root, renderers
        )
        canonical = root / "canonical.rs"
        canonical.write_text(
            "fn protocol_example_cases_matches_production() {}\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(ValueError, "non-generated owner"):
            refinement.validate_generated_selector_ownership(
                consumers, root, ("canonical.rs",)
            )

    def test_live_renderers_use_only_generated_targets(self) -> None:
        self.assertTrue(
            all("generated" in renderer.target.lower() for renderer in generator.RENDERERS.values())
        )

    def test_live_manifest_parses(self) -> None:
        consumers = refinement.parse_manifest(
            __import__("json").loads(refinement.VECTORS.read_text(encoding="utf-8")),
            refinement.MANIFEST.read_text(encoding="utf-8"),
            refinement.MODEL.read_text(encoding="utf-8"),
            refinement.IMPLEMENTATION.read_text(encoding="utf-8"),
            refinement.REFINEMENT.read_text(encoding="utf-8"),
        )
        self.assertGreater(len(consumers), 0)


if __name__ == "__main__":
    unittest.main()
