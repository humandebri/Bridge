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

    def test_symlinked_replacement_marker_fails_closed(self) -> None:
        self.install_replacement_layout()
        unrelated = self.root / "unrelated.txt"
        self.touch(unrelated)
        marker = self.policy_dir / "legacy-stack-binding.json"
        marker.unlink()
        marker.symlink_to(unrelated)

        with self.assertRaisesRegex(ValueError, "must not be symlinked"):
            classify_layout(self.root, self.script_root)

    def test_broken_upgrade_script_symlink_fails_closed(self) -> None:
        self.install_replacement_layout()
        script = self.script_root / UPGRADE_SCRIPTS[0]
        script.parent.mkdir(parents=True)
        script.symlink_to(self.root / "missing.py")

        with self.assertRaisesRegex(ValueError, "must not be symlinked"):
            classify_layout(self.root, self.script_root)

    def test_symlinked_policy_directory_fails_closed(self) -> None:
        self.install_replacement_layout()
        actual_policy_dir = self.root / "actual-policy"
        self.policy_dir.rename(actual_policy_dir)
        self.policy_dir.symlink_to(actual_policy_dir, target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "must not be symlinked"):
            classify_layout(self.root, self.script_root)

    def test_symlinked_candidate_script_root_fails_closed(self) -> None:
        self.install_upgrade_layout()
        actual_script_root = self.root / "actual-candidate-scripts"
        self.script_root.rename(actual_script_root)
        self.script_root.symlink_to(actual_script_root, target_is_directory=True)

        with self.assertRaisesRegex(ValueError, "must not be symlinked"):
            classify_layout(self.root, self.script_root)

    def test_missing_candidate_script_root_fails_closed(self) -> None:
        self.install_replacement_layout()
        self.script_root.rmdir()

        with self.assertRaisesRegex(ValueError, "must be an existing"):
            classify_layout(self.root, self.script_root)

    def test_non_directory_candidate_script_root_fails_closed(self) -> None:
        self.install_replacement_layout()
        self.script_root.rmdir()
        self.touch(self.script_root)

        with self.assertRaisesRegex(ValueError, "must be an existing"):
            classify_layout(self.root, self.script_root)

    def test_broken_obsolete_policy_symlink_fails_closed(self) -> None:
        self.install_upgrade_layout()
        obsolete = self.policy_dir / "v33-to-v34-upgrade-policy.json"
        obsolete.symlink_to(self.root / "missing.json")

        with self.assertRaisesRegex(ValueError, "must not be symlinked"):
            classify_layout(self.root, self.script_root)


if __name__ == "__main__":
    unittest.main()
