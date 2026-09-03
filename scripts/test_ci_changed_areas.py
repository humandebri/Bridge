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

    def assert_review(self, paths: list[str], expected: bool) -> None:
        self.assertEqual(ci_changed_areas.review_required(paths), expected)

    def test_docs_only_runs_no_component_gate(self) -> None:
        self.assert_areas(["docs/bridge-flow.md", "README.md", "LICENSE", ".gitignore"])

    def test_docs_only_emits_a_nonempty_workflow_matrix(self) -> None:
        with tempfile.NamedTemporaryFile() as output:
            subprocess.run(
                [
                    sys.executable,
                    ci_changed_areas.__file__,
                    "--github-output",
                    output.name,
                    "docs/bridge-flow.md",
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            output.seek(0)
            values = dict(
                line.rstrip().split("=", 1)
                for line in output.read().decode("utf-8").splitlines()
            )
        self.assertEqual(values["any"], "false")
        self.assertEqual(json.loads(values["matrix"]), ["none"])

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

    def test_ui_dependency_only_change_runs_ui(self) -> None:
        self.assert_areas(["ui/pnpm-lock.yaml"], "ui")
        self.assert_areas(["ui/src/styles.css"], "ui")

    def test_ui_real_e2e_change_runs_ui_and_real(self) -> None:
        self.assert_areas(["ui/e2e-real/bridge-real.spec.ts"], "ui", "real")

    def test_integration_ui_change_runs_real(self) -> None:
        self.assert_areas(["ui/src/lib/ic/bridge.ts"], "ui", "real")

    def test_shared_runtime_ui_changes_run_real(self) -> None:
        for path in (
            "ui/src/lib/deposit-intents.ts",
            "ui/src/lib/deposit-history.ts",
            "ui/src/lib/withdrawal-notification.ts",
            "ui/src/lib/withdrawal-submit.ts",
            "ui/src/lib/future-settlement-module.ts",
        ):
            with self.subTest(path=path):
                expected = (
                    ("ui", "real", "proofs")
                    if path == "ui/src/lib/withdrawal-submit.ts"
                    else ("ui", "real")
                )
                self.assert_areas([path], *expected)

    def test_review_is_not_required_for_docs_and_production_sources(self) -> None:
        self.assert_review(["docs/bridge-flow.md"], False)
        self.assert_review(["README.md", "AGENTS.md", "LICENSE"], False)
        self.assert_review(["canister/bridge-canister/src/api.rs"], False)
        self.assert_review(["contracts/src/Bridge.sol"], False)
        self.assert_review(["ui/src/features/bridge/bridge-page.tsx"], False)

    def test_review_is_required_for_validation_and_dependency_inputs(self) -> None:
        for path in (
            ".github/workflows/trusted-pr-gate.yml",
            ".github/README.md",
            ".gitmodules",
            "Cargo.lock",
            "canister/bridge-core/tests/protocol_vectors.rs",
            "canister/bridge-core/benches/throughput.rs",
            "contracts/test/Bridge.t.sol",
            "integration/phase3.spec.ts",
            "pnpm-lock.yaml",
            "rust-toolchain.toml",
            "scripts/ci-local.sh",
            "ui/e2e-real/bridge-real.spec.ts",
            "ui/src/lib/runtime-validation.test.ts",
            "ui/src/lib/runtime-validation.spec.ts",
            "ui/src/widget.test.jsx",
            "ui/src/bridge.e2e.ts",
            "canister/foo/src/parser_test.py",
            "ui/vite.real.config.ts",
            "ui/package.json",
            "verification/claims.tsv",
            "verification/README.md",
        ):
            with self.subTest(path=path):
                self.assert_review([path], True)

    def test_review_fails_closed_for_unknown_paths(self) -> None:
        self.assert_review(["config/new-policy.toml"], True)
        self.assert_review(["config/new-policy.md"], True)

    def test_raw_diff_detects_added_removed_or_changed_gitlinks(self) -> None:
        ordinary = (
            b":100644 100644 "
            + b"0" * 40
            + b" "
            + b"1" * 40
            + b" M\0ui/src/app.tsx\0"
        )
        added = (
            b":000000 160000 "
            + b"0" * 40
            + b" "
            + b"1" * 40
            + b" A\0ui/src/vendor\0"
        )
        removed = (
            b":160000 000000 "
            + b"1" * 40
            + b" "
            + b"0" * 40
            + b" D\0ui/src/vendor\0"
        )
        self.assertFalse(ci_changed_areas.raw_diff_has_gitlink(ordinary))
        self.assertTrue(ci_changed_areas.raw_diff_has_gitlink(added))
        self.assertTrue(ci_changed_areas.raw_diff_has_gitlink(removed))

    def test_proof_owned_runtime_validation_runs_proofs_and_real(self) -> None:
        self.assert_areas(
            ["ui/src/lib/runtime-validation.ts"],
            "proofs",
            "ui",
            "real",
        )

    def test_proof_change_runs_proofs_only(self) -> None:
        self.assert_areas(["verification/lean/Bridge.lean"], "proofs")

    def test_safety_kernel_and_proof_manifest_keep_safety_areas(self) -> None:
        self.assert_areas(
            ["tools/bridge-profile/src/main.rs", "verification/proof-impact.tsv"],
            "rust",
            "proofs",
        )

    def test_certora_only_change_runs_only_advisory_checks(self) -> None:
        for path in (
            "verification/certora/specs/Bridge.spec",
            "verification/certora/confs/Bridge.conf",
            "scripts/certora_fingerprint.py",
            "scripts/certora_results.py",
            ".github/workflows/certora-advisory.yml",
        ):
            with self.subTest(path=path):
                self.assert_areas([path], "certora")

    def test_shared_timelock_test_keeps_release_checks(self) -> None:
        self.assert_areas(
            ["contracts/test/BridgeTimelock.t.sol"],
            "contracts",
            "proofs",
        )

    def test_certora_and_production_mix_keeps_both_boundaries(self) -> None:
        self.assert_areas(
            [
                "verification/certora/specs/Bridge.spec",
                "contracts/src/Bridge.sol",
            ],
            "certora",
            "contracts",
            "proofs",
            "ui",
            "real",
        )

    def test_certora_transitive_python_dependencies_run_proofs_and_certora(self) -> None:
        for path in ci_changed_areas._certora_python_dependencies():
            with self.subTest(path=path):
                areas = ci_changed_areas.classify([path])
                self.assertTrue(areas["certora"])
                if path not in ci_changed_areas.CERTORA_ADVISORY_EXACT_PATHS:
                    self.assertTrue(areas["proofs"])

    def test_proof_owned_ui_adapter_runs_proofs_and_runtime_checks(self) -> None:
        self.assert_areas(
            ["ui/src/lib/pending-confirmations.ts"],
            "proofs",
            "ui",
            "real",
        )

    def test_ci_infrastructure_runs_every_area(self) -> None:
        self.assert_areas([".github/workflows/ci.yml"], *ci_changed_areas.AREAS)

    def test_submodule_change_runs_every_area(self) -> None:
        self.assert_areas([".gitmodules"], *ci_changed_areas.AREAS)

    def test_bridge_profile_tool_runs_rust_and_owned_proofs(self) -> None:
        self.assert_areas(["tools/bridge-profile/src/main.rs"], "rust", "proofs")

    def test_every_proof_owned_source_enables_proofs(self) -> None:
        for path in ci_changed_areas._proof_owned_paths():
            with self.subTest(path=path):
                self.assertTrue(ci_changed_areas.classify([path])["proofs"])

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

    def test_production_source_alone_remains_eligible_for_isolated_checks(self) -> None:
        self.assert_areas(
            ["canister/bridge-canister/src/api.rs"],
            "rust",
            "proofs",
            "real",
            "icp",
        )

    def test_policy_and_production_source_mix_runs_every_area(self) -> None:
        paths = [
            "scripts/ci-local.sh",
            "canister/bridge-canister/src/api.rs",
        ]
        self.assert_areas(paths, *ci_changed_areas.AREAS)

    def test_cli_emits_matrix_for_policy_source_mix(self) -> None:
        with tempfile.NamedTemporaryFile() as output:
            result = subprocess.run(
                [
                    sys.executable,
                    ci_changed_areas.__file__,
                    "--github-output",
                    output.name,
                    ".github/workflows/trusted-pr-gate.yml",
                    "contracts/src/Bridge.sol",
                ],
                env={**os.environ, "GITHUB_OUTPUT": output.name},
                capture_output=True,
                text=True,
                check=True,
            )
            output.seek(0)
            values = dict(
                line.rstrip().split("=", 1)
                for line in output.read().decode("utf-8").splitlines()
            )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(json.loads(values["matrix"]), list(ci_changed_areas.AREAS))
        self.assertEqual(values["any"], "true")
        self.assertEqual(values["review_required"], "true")


if __name__ == "__main__":
    unittest.main()
