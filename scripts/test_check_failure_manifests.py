#!/usr/bin/env python3
"""Regression tests for recursive negative-fixture coverage."""

from pathlib import Path
import tempfile
import unittest

from check_failure_manifests import relative_fixture_paths, require_unique_manifest_column


class FailureManifestTests(unittest.TestCase):
    def test_rejects_one_fixture_mapped_to_multiple_claim_premises(self) -> None:
        rows = [
            ["claim_one", "Drifted.lean", "generation_equality"],
            ["claim_two", "Drifted.lean", "signed_at_equality"],
        ]
        with self.assertRaisesRegex(ValueError, "duplicate Lean failure fixture"):
            require_unique_manifest_column(rows, 1, "Lean failure fixture")

    def test_discovers_nested_fixtures_by_relative_path(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            direct = root / "Direct.sol"
            nested = root / "nested" / "Nested.sol"
            nested.parent.mkdir()
            direct.write_text("direct", encoding="utf-8")
            nested.write_text("nested", encoding="utf-8")
            self.assertEqual(
                relative_fixture_paths(root, ".sol"),
                ["Direct.sol", "nested/Nested.sol"],
            )
            nested_lean = root / "nested" / "Nested.lean"
            nested_lean.write_text("nested", encoding="utf-8")
            self.assertEqual(
                relative_fixture_paths(root, ".lean"),
                ["nested/Nested.lean"],
            )


if __name__ == "__main__":
    unittest.main()
