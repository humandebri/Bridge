#!/usr/bin/env python3
"""Fail closed if the trusted PR gate stops separating base policy from PR code."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "trusted-pr-gate.yml"


class TrustedPrGateTests(unittest.TestCase):
    def test_trusted_bootstrap_files_are_present_and_pinned(self) -> None:
        dockerfile = ROOT / ".github" / "trusted-pr" / "Dockerfile"
        wrapper = ROOT / "scripts" / "trusted-pr-container.sh"
        self.assertTrue(dockerfile.is_file())
        self.assertTrue(wrapper.is_file())
        first_line = dockerfile.read_text(encoding="utf-8").splitlines()[0]
        self.assertRegex(first_line, r"^FROM ubuntu@sha256:[0-9a-f]{64}$")

    def test_policy_is_loaded_from_the_base_commit(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("pull_request_target:", workflow)
        self.assertIn("ref: ${{ github.event.pull_request.base.ref }}", workflow)
        self.assertIn('echo "sha=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"', workflow)
        self.assertIn("BASE_SHA: ${{ steps.base.outputs.sha }}", workflow)
        self.assertIn("base_sha: ${{ steps.base.outputs.sha }}", workflow)
        self.assertIn("ref: ${{ needs.classify.outputs.base_sha }}", workflow)
        self.assertIn(
            "python3 scripts/ci_changed_areas.py --null --github-output \"$GITHUB_OUTPUT\"",
            workflow,
        )
        self.assertIn("Require complete trusted classifier outputs", workflow)
        self.assertIn("CLASSIFY_POLICY: ${{ steps.classify.outputs.policy }}", workflow)
        self.assertIn('case "$CLASSIFY_POLICY" in', workflow)
        self.assertLess(
            workflow.index("Require complete trusted classifier outputs"),
            workflow.index("Install trusted classifier dependencies"),
        )
        self.assertLess(
            workflow.index("ref: ${{ github.event.pull_request.base.ref }}"),
            workflow.index("python3 scripts/ci_changed_areas.py"),
        )
        self.assertIn("sudo apt-get install --yes ripgrep", workflow)
        self.assertLess(
            workflow.index("sudo apt-get install --yes ripgrep"),
            workflow.index("python3 scripts/test_ci_modes.py"),
        )

    def test_untrusted_jobs_are_read_only_and_sha_pinned(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("ref: ${{ github.event.pull_request.head.sha }}", workflow)
        self.assertIn(
            "ref: ${{ github.event.pull_request.head.sha }}\n"
            "          path: source\n"
            "          fetch-depth: 0",
            workflow,
        )
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

    def test_untrusted_lifecycle_never_runs_before_policy_and_isolation_are_fixed(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("path: trusted-policy", workflow)
        self.assertIn("path: source", workflow)
        self.assertIn("trusted-policy/scripts/install-ci-tools.sh all", workflow)
        self.assertIn(
            "pnpm --dir trusted-policy/ui install --frozen-lockfile --ignore-scripts",
            workflow,
        )
        self.assertIn(
            "pnpm --dir trusted-policy install --frozen-lockfile",
            workflow,
        )
        self.assertIn("trusted-policy/.github/trusted-pr/Dockerfile", workflow)
        self.assertIn("trusted-policy/scripts/trusted-pr-container.sh source trusted-policy", workflow)
        self.assertLess(
            workflow.index("Build the pinned isolation image from trusted policy"),
            workflow.index("Check out exact untrusted head as read-only container input"),
        )
        self.assertNotIn("pnpm --dir source/ui install", workflow)

    def test_each_check_uses_fresh_read_only_container_boundaries(self) -> None:
        wrapper = (ROOT / "scripts" / "trusted-pr-container.sh").read_text(encoding="utf-8")
        self.assertIn("docker run --rm", wrapper)
        self.assertIn("--read-only", wrapper)
        self.assertIn("--network none", wrapper)
        self.assertIn("--cap-drop ALL", wrapper)
        self.assertIn("dst=/workspace,readonly", wrapper)
        self.assertIn("dst=/workspace/scripts,readonly", wrapper)
        self.assertIn("dst=/workspace/node_modules,readonly", wrapper)
        self.assertIn("dst=/workspace/ui/node_modules,readonly", wrapper)
        self.assertIn("dst=/workspace/ui/node_modules/.tmp", wrapper)
        self.assertIn("dst=/workspace/ui/.e2e-runtime", wrapper)
        self.assertIn('if [[ "$MODE" == "real" ]]', wrapper)
        self.assertIn("dst=/workspace/.tools,readonly", wrapper)
        self.assertIn("BRIDGE_EXPECTED_HEAD_SHA", wrapper)
        self.assertNotIn("src=/home/runner,dst=/home/runner", wrapper)
        self.assertIn(".cargo .rustup .local .elan .foundry setup-pnpm", wrapper)
        self.assertIn("/home/runner/.cache/ms-playwright", wrapper)
        self.assertNotIn("GITHUB_TOKEN", wrapper)
        self.assertNotIn("GH_TOKEN", wrapper)

    def test_policy_changes_require_current_codeowner_approval_and_untrusted_tests(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        codeowners = (ROOT / ".github" / "CODEOWNERS").read_text(encoding="utf-8")
        self.assertIn("policy: ${{ steps.classify.outputs.policy }}", workflow)
        self.assertIn(
            "if: needs.classify.outputs.any == 'true'",
            workflow,
        )
        self.assertIn("needs.classify.outputs.policy == 'true'", workflow)
        self.assertIn("HEAD_SHA: ${{ github.event.pull_request.head.sha }}", workflow)
        self.assertIn('select(.state == "APPROVED" and .commit_id == $head)', workflow)
        self.assertIn(".commit_id == $head", workflow)
        self.assertIn('any(. == "humandebri")', workflow)
        classifier = (ROOT / "scripts" / "ci_changed_areas.py").read_text(encoding="utf-8")
        self.assertNotIn("validate_policy_only", classifier)
        self.assertNotIn("policy-changing PRs must be policy-only", classifier)
        for path in ("/.github/", "/scripts/", "/Cargo.lock", "/verification/"):
            self.assertIn(path, codeowners)

    def test_aggregate_gate_requires_each_applicable_job_to_succeed(self) -> None:
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn('if [[ "$ANY" == true ]]; then', workflow)
        self.assertIn('if [[ "$POLICY" == true ]]; then', workflow)
        self.assertIn('test "$TEST_RESULT" = success', workflow)
        self.assertIn('test "$POLICY_REVIEW_RESULT" = success', workflow)
        self.assertNotIn(
            'test "$TEST_RESULT" = success -o "$TEST_RESULT" = skipped', workflow
        )
        self.assertNotIn(
            'test "$POLICY_REVIEW_RESULT" = success -o "$POLICY_REVIEW_RESULT" = skipped',
            workflow,
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

    def test_trusted_driver_accepts_the_reviewed_pr2_layout(self) -> None:
        driver = (ROOT / "scripts" / "ci-local.sh").read_text(encoding="utf-8")
        for allowed_storage_file in (
            "browser-lock.ts",
            "browser-lock.test.ts",
            "risk-acknowledgement.tsx",
            "risk-acknowledgement.test.tsx",
        ):
            self.assertIn(f"--glob '!{allowed_storage_file}'", driver)
        self.assertGreaterEqual(driver.count("--ignored-error-codes 2394"), 2)
        self.assertIn('(cd "$ROOT/verification/lean" && lake build)', driver)
        self.assertIn("      executable)", driver)
        self.assertIn(
            "Verus executable obligation does not call production symbol", driver
        )
        self.assertIn(
            """awk -F $'\\t' '$1 != "executable" { print $2 }'""", driver
        )

    def test_pr_controlled_gate_does_not_return(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("pull_request:", workflow)
        self.assertNotIn("pr-gate:", workflow)
        self.assertNotIn("ci_changed_areas.py", workflow)


if __name__ == "__main__":
    unittest.main()
