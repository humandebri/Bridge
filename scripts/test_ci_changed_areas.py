#!/usr/bin/env python3
"""Regression tests for change-to-CI-area classification."""

import unittest

import ci_changed_areas


class ChangedAreaTests(unittest.TestCase):
    def assert_areas(self, paths: list[str], *expected: str) -> None:
        actual = {area for area, enabled in ci_changed_areas.classify(paths).items() if enabled}
        self.assertEqual(actual, set(expected))

    def test_docs_only_runs_no_component_gate(self) -> None:
        self.assert_areas(["docs/bridge-flow.md"])

    def test_rust_change_runs_rust_real_and_icp(self) -> None:
        self.assert_areas(
            ["canister/bridge-canister/src/api.rs"],
            "rust",
            "real",
            "icp",
        )

    def test_contract_change_runs_contract_ui_and_real(self) -> None:
        self.assert_areas(
            ["contracts/src/Bridge.sol"],
            "contracts",
            "ui",
            "real",
        )

    def test_visual_ui_change_avoids_real_integration(self) -> None:
        self.assert_areas(["ui/src/styles.css"], "ui")

    def test_integration_ui_change_runs_real(self) -> None:
        self.assert_areas(["ui/src/lib/ic/bridge.ts"], "ui", "real")

    def test_proof_change_runs_proofs_only(self) -> None:
        self.assert_areas(["verification/lean/Bridge.lean"], "proofs")

    def test_ci_infrastructure_runs_every_area(self) -> None:
        self.assert_areas([".github/workflows/ci.yml"], *ci_changed_areas.AREAS)

    def test_submodule_change_runs_every_area(self) -> None:
        self.assert_areas([".gitmodules"], *ci_changed_areas.AREAS)


if __name__ == "__main__":
    unittest.main()
