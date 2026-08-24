#!/usr/bin/env python3
"""Regression tests for the Halmos lock-to-environment binding."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import halmos_environment


class HalmosEnvironmentTests(unittest.TestCase):
    def test_lock_fingerprint_changes_with_either_input(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            project = root / "verification" / "halmos"
            project.mkdir(parents=True)
            pyproject = project / "pyproject.toml"
            lock = project / "uv.lock"
            pyproject.write_text("before\n", encoding="utf-8")
            lock.write_text("locked\n", encoding="utf-8")
            with (
                patch.object(halmos_environment, "ROOT", root),
                patch.object(halmos_environment, "PROJECT", project),
                patch.object(halmos_environment, "INPUTS", (pyproject, lock)),
            ):
                before = halmos_environment.lock_fingerprint()
                lock.write_text("changed\n", encoding="utf-8")
                after = halmos_environment.lock_fingerprint()
            self.assertNotEqual(before, after)

    def test_stamp_shape_is_exact(self) -> None:
        value = halmos_environment.lock_fingerprint()
        self.assertEqual(value["schema"], 1)
        self.assertEqual(value["algorithm"], "sha256")
        self.assertEqual(len(value["digest"]), 64)
        self.assertEqual(json.loads(json.dumps(value)), value)


if __name__ == "__main__":
    unittest.main()
