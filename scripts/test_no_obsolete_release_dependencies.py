#!/usr/bin/env python3
"""Regression tests for the obsolete active dependency guard."""

from __future__ import annotations

import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import check_no_obsolete_release_dependencies as guard


class ObsoleteReleaseDependencyTests(unittest.TestCase):
    def violations(self, root: Path) -> list[str]:
        # Temporary fixtures intentionally omit the real candidate-scripts tree
        # that trusted execution uses for the checked-out repository.
        with mock.patch.dict(os.environ, clear=False):
            os.environ.pop("BRIDGE_CANDIDATE_SCRIPTS", None)
            return guard.violations(root)

    def root(self) -> tuple[tempfile.TemporaryDirectory[str], Path]:
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        (root / "scripts").mkdir()
        (root / "verification").mkdir()
        (root / "docs/runbooks").mkdir(parents=True)
        (root / "scripts/ci-local.sh").write_text("#!/bin/sh\n", encoding="utf-8")
        return temporary, root

    def test_direct_obsolete_reference_is_rejected(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        (root / "scripts/production-release.sh").write_text(
            'scripts/plan007/capture-obsolete-pause-evidence.mjs\n', encoding="utf-8"
        )
        self.assertTrue(self.violations(root))

    def test_obsolete_reference_through_helper_is_rejected(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        (root / "scripts/production-release.sh").write_text(
            'python3 "$(dirname "$0")/helper.py"\n', encoding="utf-8"
        )
        (root / "scripts/helper.py").write_text(
            'POLICY = "deployments/sepolia-staging/obsolete-replacement-policy.json"\n',
            encoding="utf-8",
        )
        self.assertTrue(self.violations(root))

    def test_missing_literal_helper_fails_closed(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        (root / "scripts/production-release.sh").write_text(
            'python3 "$(dirname "$0")/missing.py"\n', encoding="utf-8"
        )
        self.assertIn("missing helper", "\n".join(self.violations(root)))

    def test_missing_test_helper_from_ci_entrypoint_fails_closed(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        (root / "scripts/ci-local.sh").write_text(
            'node "$ROOT/scripts/plan007/test-missing.mjs"\n', encoding="utf-8"
        )
        self.assertIn("missing helper", "\n".join(self.violations(root)))

    def install_replacement_layout(self, root: Path) -> None:
        policy = root / "deployments/sepolia-staging"
        policy.mkdir(parents=True)
        (policy / "legacy-stack-binding.json").write_text("{}\n", encoding="utf-8")
        (policy / "fresh-stack.template.json").write_text("{}\n", encoding="utf-8")
        shutil.copy2(
            Path(__file__).with_name("trusted_staging_layout.py"),
            root / "scripts/trusted_staging_layout.py",
        )

    def install_upgrade_layout_without_test_helper(self, root: Path) -> None:
        policy = root / "deployments/sepolia-staging"
        policy.mkdir(parents=True)
        (policy / "staging-bridge-upgrade-policy.json").write_text(
            "{}\n", encoding="utf-8"
        )
        for relative in (
            "scripts/plan007/staging_canister_upgrade.py",
            "scripts/plan007/staging-canister-upgrade.sh",
        ):
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture\n", encoding="utf-8")
        shutil.copy2(
            Path(__file__).with_name("trusted_staging_layout.py"),
            root / "scripts/trusted_staging_layout.py",
        )

    def test_missing_upgrade_test_helper_is_allowed_for_replacement_layout(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        self.install_replacement_layout(root)
        (root / "scripts/ci-local.sh").write_text(
            'python3 "$ROOT/scripts/plan007/test_staging_canister_upgrade.py"\n',
            encoding="utf-8",
        )
        self.assertEqual(self.violations(root), [])

    def test_missing_upgrade_test_helper_fails_for_upgrade_layout(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        self.install_upgrade_layout_without_test_helper(root)
        (root / "scripts/ci-local.sh").write_text(
            'python3 "$ROOT/scripts/plan007/test_staging_canister_upgrade.py"\n',
            encoding="utf-8",
        )
        self.assertIn("missing helper", "\n".join(self.violations(root)))

    def test_missing_upgrade_test_helper_fails_for_mixed_layout(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        self.install_upgrade_layout_without_test_helper(root)
        policy = root / "deployments/sepolia-staging"
        (policy / "legacy-stack-binding.json").write_text("{}\n", encoding="utf-8")
        (root / "scripts/ci-local.sh").write_text(
            'python3 "$ROOT/scripts/plan007/test_staging_canister_upgrade.py"\n',
            encoding="utf-8",
        )
        self.assertIn("missing helper", "\n".join(self.violations(root)))

    def test_other_missing_test_helper_still_fails_for_replacement_layout(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        self.install_replacement_layout(root)
        (root / "scripts/ci-local.sh").write_text(
            'python3 "$ROOT/scripts/plan007/test-other.mjs"\n',
            encoding="utf-8",
        )
        self.assertIn("missing helper", "\n".join(self.violations(root)))

    def test_unreachable_historical_asset_is_allowed(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        historical = root / "deployments/sepolia-staging"
        historical.mkdir(parents=True)
        (historical / "obsolete-replacement-policy.json").write_text(
            "{}\n", encoding="utf-8"
        )
        (root / "scripts/production-release.sh").write_text(
            "#!/bin/sh\n", encoding="utf-8"
        )
        self.assertEqual(self.violations(root), [])


if __name__ == "__main__":
    unittest.main()
