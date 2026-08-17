#!/usr/bin/env python3
"""Fail closed if the trusted PR gate stops separating base policy from PR code."""

from pathlib import Path
import os
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOW = ROOT / ".github" / "workflows" / "trusted-pr-gate.yml"


class TrustedPrGateTests(unittest.TestCase):
    def test_trusted_bootstrap_files_are_present_and_pinned(self) -> None:
        dockerfile = ROOT / ".github" / "trusted-pr" / "Dockerfile"
        wrapper = ROOT / "scripts" / "trusted-pr-container.sh"
        mountpoints = ROOT / "scripts" / "trusted-pr-mountpoints.sh"
        mount_smoke = ROOT / "scripts" / "test_trusted_pr_container_mounts.sh"
        self.assertTrue(dockerfile.is_file())
        self.assertTrue(wrapper.is_file())
        self.assertTrue(mountpoints.is_file())
        self.assertTrue(mount_smoke.is_file())
        self.assertTrue(os.access(mountpoints, os.X_OK))
        self.assertTrue(os.access(mount_smoke, os.X_OK))
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
            "cargo fetch --locked --manifest-path trusted-policy/Cargo.toml",
            workflow,
        )
        self.assertIn(
            "working-directory: trusted-policy",
            workflow,
        )
        self.assertIn(
            "icp project show --project-root-override .",
            workflow,
        )
        self.assertIn("ICP_CLI_DISABLE_UPDATE: \"1\"", workflow)
        self.assertIn("ICP_TELEMETRY_DISABLED: \"1\"", workflow)
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
        self.assertIn(
            "node trusted-policy/ui/scripts/download-ledger-artifacts.mjs",
            workflow,
        )
        for prefetch in (
            "Prefetch locked Rust dependencies from trusted policy",
            "Prefetch pinned ICP recipes from trusted policy",
            "Prefetch verified real-E2E ledger artifacts",
        ):
            self.assertLess(
                workflow.index(prefetch),
                workflow.index("Check out exact untrusted head as read-only container input"),
            )
        self.assertLess(
            workflow.index("Build the pinned isolation image from trusted policy"),
            workflow.index("Check out exact untrusted head as read-only container input"),
        )
        self.assertNotIn("pnpm --dir source/ui install", workflow)

    def test_each_check_uses_fresh_read_only_container_boundaries(self) -> None:
        wrapper = (ROOT / "scripts" / "trusted-pr-container.sh").read_text(encoding="utf-8")
        self.assertIn("docker run --rm", wrapper)
        self.assertIn('--user "$(id -u):$(id -g)"', wrapper)
        self.assertIn("--read-only", wrapper)
        self.assertIn("--network none", wrapper)
        self.assertIn("--cap-drop ALL", wrapper)
        self.assertIn("dst=/workspace,readonly", wrapper)
        self.assertIn("dst=/workspace/scripts,readonly", wrapper)
        self.assertIn("dst=/workspace/node_modules,readonly", wrapper)
        self.assertIn("dst=/workspace/ui/node_modules,readonly", wrapper)
        self.assertIn("dst=/workspace/ui/node_modules/.tmp", wrapper)
        self.assertIn("dst=/workspace/ui/node_modules/.vite-temp", wrapper)
        self.assertIn("dst=/workspace/ui/.e2e-runtime", wrapper)
        self.assertIn("dst=/workspace/ui/.e2e-cache,readonly", wrapper)
        self.assertIn("dst=/workspace/verification/output", wrapper)
        self.assertIn("dst=/workspace/verification/lean/.lake", wrapper)
        self.assertIn("dst=/workspace/.icp/cache", wrapper)
        self.assertNotIn("dst=/workspace/.local", wrapper)
        self.assertIn('source "$POLICY_ROOT/scripts/trusted-pr-mountpoints.sh"', wrapper)
        self.assertIn("bridge_prepare_candidate_mountpoint", wrapper)
        self.assertIn("bridge_cleanup_mountpoints", wrapper)
        self.assertIn('if [[ "$MODE" == "real" ]]', wrapper)
        self.assertIn("dst=/workspace/.tools,readonly", wrapper)
        self.assertIn("BRIDGE_EXPECTED_HEAD_SHA", wrapper)
        self.assertNotIn("src=/home/runner,dst=/home/runner", wrapper)
        self.assertIn(".cargo .rustup .local .elan .foundry setup-pnpm", wrapper)
        self.assertIn("/home/runner/.cache/ms-playwright", wrapper)
        self.assertIn("src=/home/runner/.svm,dst=/scratch/home/.svm,readonly", wrapper)
        self.assertIn(
            "src=/home/runner/.elan/toolchains,dst=/scratch/home/.elan/toolchains,readonly",
            wrapper,
        )
        self.assertNotIn("trusted Elan settings are missing", wrapper)
        self.assertNotIn("cp /home/runner/.elan/settings.toml", wrapper)
        self.assertIn("BRIDGE_TRUSTED_DEPS_READY=1", wrapper)
        self.assertIn("PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN=false", wrapper)
        self.assertIn("ELAN_HOME=/scratch/home/.elan", wrapper)
        self.assertIn("CARGO_NET_OFFLINE=true", wrapper)
        self.assertIn("FOUNDRY_OFFLINE=true", wrapper)
        self.assertIn("XDG_DATA_HOME=/scratch/home/.local/share", wrapper)
        self.assertIn("XDG_CONFIG_HOME=/scratch/home/.config", wrapper)
        self.assertIn(
            'cp -R /home/runner/.local/share/icp-cli/pkg/. '
            '"$SCRATCH/home/.local/share/icp-cli/pkg/"',
            wrapper,
        )
        self.assertNotIn("/home/runner/.local/share/icp-cli/identity", wrapper)
        self.assertIn("ICP_CLI_DISABLE_UPDATE=1", wrapper)
        self.assertIn("ICP_TELEMETRY_DISABLED=1", wrapper)
        self.assertNotIn("GITHUB_TOKEN", wrapper)
        self.assertNotIn("GH_TOKEN", wrapper)

        driver = (ROOT / "scripts" / "ci-local.sh").read_text(encoding="utf-8")
        self.assertIn("require_workspace_dependencies", driver)
        self.assertIn("BRIDGE_TRUSTED_DEPS_READY", driver)
        self.assertIn(
            'pnpm --dir "$ROOT/ui" exec playwright test --config playwright.real.config.ts',
            driver,
        )

        installer = (ROOT / "scripts" / "install-ci-tools.sh").read_text(encoding="utf-8")
        self.assertIn("solc-linux-amd64-v0.8.36+commit.8a079791", installer)
        self.assertIn(
            "c8d35afdddc3cd2743ee88b8f25e0fecd16e2bdd5f2120f37e52cd9cc45ae0e6",
            installer,
        )
        self.assertIn('>"$HOME/.svm/.global-version"', installer)

        dockerfile = (ROOT / ".github" / "trusted-pr" / "Dockerfile").read_text(
            encoding="utf-8"
        )
        self.assertIn("safe.directory /workspace", dockerfile)
        self.assertNotIn("safe.directory '*'", dockerfile)

    def test_candidate_scripts_are_exposed_without_overriding_trusted_checks(self) -> None:
        wrapper = (ROOT / "scripts" / "trusted-pr-container.sh").read_text(encoding="utf-8")
        consistency = (ROOT / "scripts" / "check_schema_consistency.py").read_text(
            encoding="utf-8"
        )
        self.assertIn('[[ -e "$POLICY_ROOT/$candidate_script" ]] && continue', wrapper)
        self.assertIn(
            'src=$SOURCE_ROOT/$candidate_script,dst=/workspace/$candidate_script,readonly',
            wrapper,
        )
        self.assertIn("BRIDGE_CANDIDATE_SCRIPTS=/scratch/candidate-scripts", wrapper)
        self.assertIn(
            'src=$SOURCE_ROOT/scripts,dst=/scratch/candidate-scripts,readonly',
            wrapper,
        )
        self.assertLess(
            wrapper.index("dst=/workspace/scripts,readonly"),
            wrapper.index("dst=/scratch/candidate-scripts,readonly"),
        )
        self.assertIn("BRIDGE_CANDIDATE_SCRIPTS", consistency)
        self.assertIn('relative.startswith("scripts/")', consistency)

    def test_schema_consistency_reads_isolated_candidate_scripts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory) / "candidate-scripts"
            (candidate / "plan007").mkdir(parents=True)
            (candidate / "test_production_drivers.sh").write_text(
                '{"schema_version":99,"expected_bridge_signer":[0]}\n',
                encoding="utf-8",
            )
            (candidate / "plan007" / "generate-local-e2e.mjs").write_text(
                "CURRENT_STABLE_SCHEMA_VERSION = 99\n",
                encoding="utf-8",
            )
            (candidate / "plan007" / "sepolia_e2e.py").write_text(
                "CURRENT_STABLE_SCHEMA = 99\n",
                encoding="utf-8",
            )
            env = os.environ.copy()
            env["BRIDGE_CANDIDATE_SCRIPTS"] = str(candidate)
            result = subprocess.run(
                ["python3", str(ROOT / "scripts" / "check_schema_consistency.py")],
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0, result.stderr)
            self.assertIn("stable schema mismatch", result.stderr)

    def test_mountpoint_preparation_creates_and_cleans_only_missing_directories(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            candidate = Path(directory) / "candidate"
            (candidate / "ui").mkdir(parents=True)
            subprocess.run(["git", "init", "--quiet", candidate], check=True)
            command = f"""
                set -euo pipefail
                source {ROOT / 'scripts' / 'trusted-pr-mountpoints.sh'}
                bridge_prepare_candidate_mountpoint "$1" ui/node_modules/.tmp
                test -d "$1/ui/node_modules/.tmp"
                bridge_cleanup_mountpoints
                test -d "$1/ui"
                test ! -e "$1/ui/node_modules"
            """
            subprocess.run(["bash", "-c", command, "bash", candidate], check=True)

    def test_mountpoint_preparation_rejects_links_files_and_tracked_content(self) -> None:
        helper = ROOT / "scripts" / "trusted-pr-mountpoints.sh"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for kind in ("intermediate-link", "leaf-link", "file", "tracked"):
                with self.subTest(kind=kind):
                    candidate = root / kind
                    candidate.mkdir()
                    subprocess.run(["git", "init", "--quiet", candidate], check=True)
                    if kind == "intermediate-link":
                        (candidate / "outside").mkdir()
                        (candidate / "ui").symlink_to(candidate / "outside", target_is_directory=True)
                        relative = "ui/node_modules"
                    else:
                        (candidate / "ui").mkdir()
                        relative = "ui/node_modules"
                        if kind == "leaf-link":
                            (candidate / "outside").mkdir()
                            (candidate / relative).symlink_to(candidate / "outside", target_is_directory=True)
                        elif kind == "file":
                            (candidate / relative).write_text("not a directory", encoding="utf-8")
                        else:
                            (candidate / relative).mkdir()
                            tracked = candidate / relative / "tracked.txt"
                            tracked.write_text("tracked", encoding="utf-8")
                            subprocess.run(
                                ["git", "-C", candidate, "add", "--force", relative],
                                check=True,
                            )
                    command = f"""
                        set -euo pipefail
                        source {helper}
                        bridge_prepare_candidate_mountpoint "$1" "$2"
                    """
                    result = subprocess.run(
                        ["bash", "-c", command, "bash", candidate, relative],
                        stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE,
                        text=True,
                    )
                    self.assertNotEqual(result.returncode, 0, result.stderr)

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
