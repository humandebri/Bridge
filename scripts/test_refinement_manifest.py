#!/usr/bin/env python3
"""Regression tests for refinement manifest validation and exact-one execution."""

from __future__ import annotations

import subprocess
import tempfile
import unittest
from pathlib import Path

import check_refinement_manifest as refinement


class RefinementManifestTests(unittest.TestCase):
    def fixture(self) -> tuple[Path, dict[str, object], str, str, str, str]:
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        target = root / "canister/bridge-core/tests/protocol_vectors.rs"
        target.parent.mkdir(parents=True)
        target.write_text(
            "fn protocol_example_cases_matches_production() {\n"
            "    for case in vectors().example_cases { example_kernel(case); }\n"
            "}\n",
            encoding="utf-8",
        )
        document = {
            "schema_version": 3,
            "example_cases": [{"accepted": True}],
            "example_count": 1,
        }
        manifest = (
            "example_cases\texample\texampleImpl\texample_refinement\t"
            "rust\tcanister/bridge-core/tests/protocol_vectors.rs\t"
            "protocol_example_cases_matches_production\texample_kernel"
        )
        model = "def example : Bool := true\n"
        implementation = "def exampleImpl : Bool := true\n"
        theorem = (
            "theorem example_refinement : exampleImpl = example := by\n  rfl\n"
        )
        return root, document, manifest, model, implementation, theorem

    def test_manifest_requires_exact_vector_section_coverage(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        document["missing_cases"] = [{"accepted": False}]
        document["missing_count"] = 1
        with self.assertRaisesRegex(ValueError, "do not match vectors"):
            refinement.parse_manifest(
                document, manifest, model, implementation, theorem, root
            )

    def test_manifest_rejects_missing_selector(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        manifest = manifest.replace(
            "protocol_example_cases_matches_production", "missing_selector"
        )
        with self.assertRaisesRegex(ValueError, "selector is missing"):
            refinement.parse_manifest(
                document, manifest, model, implementation, theorem, root
            )

    def test_manifest_rejects_legacy_seven_column_row(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        manifest = manifest.rsplit("\t", 1)[0]
        with self.assertRaisesRegex(ValueError, "invalid refinement manifest row"):
            refinement.parse_manifest(
                document, manifest, model, implementation, theorem, root
            )

    def test_manifest_rejects_consumer_without_vector_section(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        target = root / "canister/bridge-core/tests/protocol_vectors.rs"
        target.write_text(
            "fn protocol_example_cases_matches_production() {\n"
            "    example_kernel(true);\n"
            "}\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "does not consume section"):
            refinement.parse_manifest(
                document, manifest, model, implementation, theorem, root
            )

    def test_manifest_rejects_consumer_without_production_call(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        target = root / "canister/bridge-core/tests/protocol_vectors.rs"
        target.write_text(
            "fn protocol_example_cases_matches_production() {\n"
            "    for case in vectors().example_cases { assert!(case.accepted); }\n"
            "}\n",
            encoding="utf-8",
        )
        with self.assertRaisesRegex(ValueError, "does not call production symbol"):
            refinement.parse_manifest(
                document, manifest, model, implementation, theorem, root
            )

    def test_manifest_rejects_unrelated_selector(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        target = root / "canister/bridge-core/tests/protocol_vectors.rs"
        target.write_text(
            target.read_text(encoding="utf-8")
            + "fn unrelated() { unrelated_kernel(); }\n",
            encoding="utf-8",
        )
        manifest = manifest.replace(
            "protocol_example_cases_matches_production", "unrelated"
        )
        with self.assertRaisesRegex(ValueError, "does not consume section"):
            refinement.parse_manifest(
                document, manifest, model, implementation, theorem, root
            )

    def test_manifest_accepts_multiple_bound_consumers_for_one_section(self) -> None:
        root, document, manifest, model, implementation, theorem = self.fixture()
        target = root / "contracts/test/ProtocolVectors.t.sol"
        target.parent.mkdir(parents=True)
        target.write_text(
            "contract ProtocolVectorsTest {\n"
            "    function test_example() public {\n"
            '        string memory base = ".example_cases[";\n'
            "        example_solidity_kernel();\n"
            "    }\n"
            "}\n",
            encoding="utf-8",
        )
        manifest += (
            "\nexample_cases\texample\texampleImpl\texample_refinement\t"
            "foundry\tcontracts/test/ProtocolVectors.t.sol\t"
            "test_example\texample_solidity_kernel"
        )
        self.assertEqual(
            len(
                refinement.parse_manifest(
                    document, manifest, model, implementation, theorem, root
                )
            ),
            2,
        )

    def test_rust_consumer_requires_one_passing_test(self) -> None:
        consumer = refinement.Consumer(
            "example_cases",
            "example",
            "exampleImpl",
            "example_refinement",
            "rust",
            "canister/bridge-core/tests/protocol_vectors.rs",
            "protocol_example_cases_matches_production",
            "example_kernel",
        )

        def runner(*_args: object, **_kwargs: object) -> subprocess.CompletedProcess[str]:
            return subprocess.CompletedProcess([], 0, "running 0 tests\n", "")

        with self.assertRaisesRegex(ValueError, "did not pass exactly once"):
            refinement.execute_consumer(consumer, Path("."), runner)

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
