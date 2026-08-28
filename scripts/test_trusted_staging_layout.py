#!/usr/bin/env python3

from pathlib import Path
import tempfile
import unittest

from trusted_staging_layout import UPGRADE_SCRIPTS, classify_layout


class TrustedStagingLayoutTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary_directory.cleanup)
        self.root = Path(self.temporary_directory.name)
        self.policy_dir = self.root / "deployments" / "sepolia-staging"
        self.script_root = self.root / "candidate-scripts"
        self.policy_dir.mkdir(parents=True)
        self.script_root.mkdir()

    @staticmethod
    def touch(path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("fixture\n", encoding="utf-8")

    def install_upgrade_layout(self) -> None:
        self.touch(self.policy_dir / "staging-bridge-upgrade-policy.json")
        for relative in UPGRADE_SCRIPTS:
            self.touch(self.script_root / relative)

    def install_replacement_layout(self) -> None:
        self.touch(self.policy_dir / "legacy-stack-binding.json")
        self.touch(self.policy_dir / "fresh-stack.template.json")

    def test_complete_upgrade_layout_requires_upgrade_tests(self) -> None:
        self.install_upgrade_layout()
        self.assertEqual(classify_layout(self.root, self.script_root), "upgrade")

    def test_complete_replacement_layout_skips_obsolete_upgrade_tests(self) -> None:
        self.install_replacement_layout()
        self.assertEqual(classify_layout(self.root, self.script_root), "replacement")

    def test_mixed_layout_fails_closed(self) -> None:
        self.install_upgrade_layout()
        self.touch(self.policy_dir / "legacy-stack-binding.json")
        with self.assertRaisesRegex(ValueError, "incomplete or mixed"):
            classify_layout(self.root, self.script_root)

    def test_partial_replacement_layout_fails_closed(self) -> None:
        self.touch(self.policy_dir / "legacy-stack-binding.json")
        with self.assertRaisesRegex(ValueError, "incomplete or mixed"):
            classify_layout(self.root, self.script_root)


if __name__ == "__main__":
    unittest.main()
