#!/usr/bin/env python3
"""Fail closed if the trusted PR gate stops separating base policy from PR code."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "trusted-pr-gate.yml"


class TrustedPrGateTests(unittest.TestCase):
    def test_policy_is_loaded_from_the_base_commit(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("pull_request_target:", workflow)
        self.assertIn("ref: ${{ github.event.pull_request.base.sha }}", workflow)
        self.assertIn(
            "python3 scripts/ci_changed_areas.py --null --github-output \"$GITHUB_OUTPUT\"",
            workflow,
        )
        self.assertLess(
            workflow.index("ref: ${{ github.event.pull_request.base.sha }}"),
            workflow.index("python3 scripts/ci_changed_areas.py"),
        )

    def test_untrusted_jobs_are_read_only_and_sha_pinned(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("ref: ${{ github.event.pull_request.head.sha }}", workflow)
        self.assertGreaterEqual(workflow.count("persist-credentials: false"), 2)
        self.assertNotIn("secrets.", workflow)
        for action in (
            "actions/checkout",
            "actions/setup-node",
            "pnpm/action-setup",
            "foundry-rs/foundry-toolchain",
        ):
            line = next(line for line in workflow.splitlines() if f"uses: {action}@" in line)
            revision = line.split("@", 1)[1].split()[0]
            self.assertRegex(revision, r"^[0-9a-f]{40}$")

    def test_trusted_driver_is_installed_without_replacing_head_scripts(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("path: trusted-policy", workflow)
        self.assertIn("path: source", workflow)
        self.assertIn("trusted-policy/scripts/install-ci-tools.sh all", workflow)
        self.assertNotIn("rm -rf source/scripts", workflow)
        self.assertIn(
            "cp trusted-policy/scripts/ci-local.sh source/scripts/ci-local.trusted.sh",
            workflow,
        )
        self.assertIn("rust) scripts/ci-local.trusted.sh rust", workflow)
        self.assertIn("contracts) scripts/ci-local.trusted.sh contracts", workflow)
        self.assertIn("ui) scripts/ci-local.trusted.sh ui", workflow)
        self.assertLess(
            workflow.index(
                "cp trusted-policy/scripts/ci-local.sh source/scripts/ci-local.trusted.sh"
            ),
            workflow.index('case "$AREA" in'),
        )

    def test_ci_sensitive_paths_fail_closed_to_the_full_matrix(self) -> None:
        import ci_changed_areas

        malicious_paths = [
            ".github/workflows/trusted-pr-gate.yml",
            "scripts/ci_changed_areas.py",
            "scripts/test_ci_changed_areas.py",
            "Cargo.lock",
            "pnpm-lock.yaml",
            ".gitmodules",
            "unknown/security-policy.toml",
        ]
        for path in malicious_paths:
            with self.subTest(path=path):
                self.assertTrue(all(ci_changed_areas.classify([path]).values()))


if __name__ == "__main__":
    unittest.main()
