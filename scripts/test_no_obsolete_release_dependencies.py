#!/usr/bin/env python3
"""Regression tests for the obsolete active dependency guard."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import check_no_obsolete_release_dependencies as guard


class ObsoleteReleaseDependencyTests(unittest.TestCase):
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
        self.assertTrue(guard.violations(root))

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
        self.assertTrue(guard.violations(root))

    def test_missing_literal_helper_fails_closed(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        (root / "scripts/production-release.sh").write_text(
            'python3 "$(dirname "$0")/missing.py"\n', encoding="utf-8"
        )
        self.assertIn("missing helper", "\n".join(guard.violations(root)))

    def test_missing_test_helper_from_ci_entrypoint_fails_closed(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        (root / "scripts/ci-local.sh").write_text(
            'node "$ROOT/scripts/plan007/test-missing.mjs"\n', encoding="utf-8"
        )
        self.assertIn("missing helper", "\n".join(guard.violations(root)))

    def test_replaced_upgrade_helper_is_not_required(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        policy_dir = root / "deployments/sepolia-staging"
        policy_dir.mkdir(parents=True)
        (policy_dir / "legacy-stack-binding.json").write_text("{}\n", encoding="utf-8")
        (policy_dir / "fresh-stack.template.json").write_text("{}\n", encoding="utf-8")
        (root / "scripts/ci-local.sh").write_text(
            'python3 "$ROOT/scripts/plan007/test_staging_canister_upgrade.py"\n',
            encoding="utf-8",
        )

        self.assertEqual(guard.violations(root), [])

    def test_incomplete_replacement_still_requires_upgrade_helper(self) -> None:
        temporary, root = self.root()
        self.addCleanup(temporary.cleanup)
        policy_dir = root / "deployments/sepolia-staging"
        policy_dir.mkdir(parents=True)
        (policy_dir / "legacy-stack-binding.json").write_text("{}\n", encoding="utf-8")
        (root / "scripts/ci-local.sh").write_text(
            'python3 "$ROOT/scripts/plan007/test_staging_canister_upgrade.py"\n',
            encoding="utf-8",
        )

        self.assertIn("missing helper", "\n".join(guard.violations(root)))

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
        self.assertEqual(guard.violations(root), [])


if __name__ == "__main__":
    unittest.main()
