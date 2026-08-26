#!/usr/bin/env python3

from pathlib import Path
import os
import tempfile
import unittest
from unittest.mock import patch

from trusted_execution_context import require_trusted_execution_context


class TrustedExecutionContextTests(unittest.TestCase):
    def test_local_execution_does_not_require_a_profile_or_expected_sha(self) -> None:
        for environment in ({}, {"CI": ""}, {"CI": "false"}, {"CI": "1"}):
            with self.subTest(environment=environment), patch.dict(
                os.environ, environment, clear=True
            ):
                self.assertIsNone(require_trusted_execution_context(Path("/missing")))

    def test_ci_requires_a_lowercase_full_sha(self) -> None:
        invalid_contexts = (
            ("true", ""),
            ("true", "abc123"),
            ("true", "A" * 40),
            ("true", "g" * 40),
        )
        for ci, expected in invalid_contexts:
            with self.subTest(ci=ci, expected=expected), patch.dict(
                os.environ,
                {"CI": ci, "BRIDGE_EXPECTED_HEAD_SHA": expected},
                clear=True,
            ):
                with self.assertRaisesRegex(ValueError, "lowercase full Git SHA"):
                    require_trusted_execution_context(Path("/missing"))

    def test_ci_accepts_only_the_checked_out_head(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self._git(root, "init")
            (root / "input.txt").write_text("candidate\n", encoding="utf-8")
            self._git(root, "add", "input.txt")
            self._git(
                root,
                "-c",
                "user.name=Trusted Context Test",
                "-c",
                "user.email=trusted-context@example.invalid",
                "commit",
                "-m",
                "test fixture",
            )
            head = self._git(root, "rev-parse", "HEAD")
            with patch.dict(
                os.environ,
                {"CI": "true", "BRIDGE_EXPECTED_HEAD_SHA": head},
                clear=True,
            ):
                self.assertEqual(require_trusted_execution_context(root), head)
            with patch.dict(
                os.environ,
                {"CI": "true", "BRIDGE_EXPECTED_HEAD_SHA": "0" * 40},
                clear=True,
            ):
                with self.assertRaisesRegex(ValueError, "checkout HEAD differs"):
                    require_trusted_execution_context(root)

    @staticmethod
    def _git(root: Path, *arguments: str) -> str:
        import subprocess

        result = subprocess.run(
            ["git", "-C", str(root), *arguments],
            check=True,
            capture_output=True,
            text=True,
        )
        return result.stdout.strip()


if __name__ == "__main__":
    unittest.main()
