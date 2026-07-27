#!/usr/bin/env python3
"""Regression tests for change-to-CI-area classification."""

import unittest
import json
import os
import subprocess
import sys
import tempfile

import ci_changed_areas


class ChangedAreaTests(unittest.TestCase):
    def assert_areas(self, paths: list[str], *expected: str) -> None:
        actual = {area for area, enabled in ci_changed_areas.classify(paths).items() if enabled}
        self.assertEqual(actual, set(expected))

    def test_docs_only_runs_no_component_gate(self) -> None:
        self.assert_areas(["docs/bridge-flow.md", "README.md", "LICENSE", ".gitignore"])

    def test_rust_change_runs_rust_real_and_icp(self) -> None:
        self.assert_areas(
            ["canister/bridge-canister/src/api.rs"],
            "rust",
            "proofs",
            "real",
            "icp",
        )

    def test_contract_change_runs_contract_ui_and_real(self) -> None:
        self.assert_areas(
            ["contracts/src/Bridge.sol"],
            "contracts",
            "proofs",
            "ui",
            "real",
        )

    def test_visual_ui_change_avoids_real_integration(self) -> None:
        self.assert_areas(["ui/src/styles.css"], "ui")

    def test_integration_ui_change_runs_real(self) -> None:
        self.assert_areas(["ui/src/lib/ic/bridge.ts"], "ui", "real")

    def test_shared_runtime_ui_changes_run_real(self) -> None:
        for path in (
            "ui/src/lib/pending-confirmations.ts",
            "ui/src/lib/deposit-intents.ts",
            "ui/src/lib/deposit-history.ts",
            "ui/src/lib/withdrawal-notification.ts",
            "ui/src/lib/withdrawal-submit.ts",
            "ui/src/lib/runtime-validation.ts",
            "ui/src/lib/future-settlement-module.ts",
        ):
            with self.subTest(path=path):
                self.assert_areas([path], "ui", "real")

    def test_proof_change_runs_proofs_only(self) -> None:
        self.assert_areas(["verification/lean/Bridge.lean"], "proofs")

    def test_ci_infrastructure_runs_every_area(self) -> None:
        self.assert_areas([".github/workflows/ci.yml"], *ci_changed_areas.AREAS)

    def test_submodule_change_runs_every_area(self) -> None:
        self.assert_areas([".gitmodules"], *ci_changed_areas.AREAS)

    def test_bridge_profile_tool_runs_rust(self) -> None:
        self.assert_areas(["tools/bridge-profile/src/main.rs"], "rust")

    def test_deployment_profiles_and_icp_mappings_run_every_area(self) -> None:
        self.assert_areas(
            [
                "deployments/sepolia-staging/frontend-profile.json",
                ".icp/data/mappings/sepolia-staging.ids.json",
            ],
            *ci_changed_areas.AREAS,
        )

    def test_unknown_non_documentation_path_runs_every_area(self) -> None:
        self.assert_areas(["config/new-policy.toml"], *ci_changed_areas.AREAS)

    def test_unknown_path_mixed_with_docs_runs_every_area(self) -> None:
        self.assert_areas(
            ["docs/bridge-flow.md", "config/new-policy.toml"],
            *ci_changed_areas.AREAS,
        )

    def test_github_output_contains_enabled_area_matrix(self) -> None:
        with tempfile.NamedTemporaryFile() as output:
            subprocess.run(
                [
                    sys.executable,
                    ci_changed_areas.__file__,
                    "--github-output",
                    output.name,
                    "canister/bridge-canister/src/api.rs",
                ],
                check=True,
                env={**os.environ, "GITHUB_OUTPUT": output.name},
                capture_output=True,
                text=True,
            )
            output.seek(0)
            values = dict(
                line.rstrip().split("=", 1)
                for line in output.read().decode("utf-8").splitlines()
            )
        self.assertEqual(
            json.loads(values["matrix"]),
            ["rust", "proofs", "real", "icp"],
        )


if __name__ == "__main__":
    unittest.main()
