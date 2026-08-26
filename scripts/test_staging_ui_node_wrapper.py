#!/usr/bin/env python3
"""Regression tests for the staging UI exact-Node entrypoint."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
WRAPPER = ROOT / "ui" / "scripts" / "run-staging-assets.sh"
STAGING_ASSETS = ROOT / "ui" / "scripts" / "staging-assets.mjs"
REQUIRED_VERSION = (ROOT / ".node-version").read_text(encoding="utf-8").strip()


class StagingUiNodeWrapperTests(unittest.TestCase):
    def test_matching_node_runs_staging_assets_without_fnm(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            environment, log = self._environment(Path(temporary), with_fnm=False)
            environment["FAKE_NODE_VERSION"] = f"v{REQUIRED_VERSION}"
            result = subprocess.run(
                [WRAPPER, "verify", "/tmp/receipt.json"],
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                log.read_text(encoding="utf-8").splitlines(),
                [f"node:v{REQUIRED_VERSION}:{STAGING_ASSETS} verify /tmp/receipt.json"],
            )

    def test_mismatched_node_reexecutes_itself_with_exact_fnm_version(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            environment, log = self._environment(Path(temporary), with_fnm=True)
            environment["FAKE_NODE_VERSION"] = "v24.19.0"
            result = subprocess.run(
                [WRAPPER, "deploy", "/tmp/receipt.json"],
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                log.read_text(encoding="utf-8").splitlines(),
                [
                    f"fnm:exec --using {REQUIRED_VERSION} -- {WRAPPER} deploy /tmp/receipt.json",
                    f"node:v{REQUIRED_VERSION}:{STAGING_ASSETS} deploy /tmp/receipt.json",
                ],
            )

    def test_mismatched_node_fails_closed_when_fnm_is_missing(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            environment, _ = self._environment(Path(temporary), with_fnm=False)
            environment["FAKE_NODE_VERSION"] = "v24.19.0"
            result = subprocess.run(
                [WRAPPER, "generate", "/tmp/receipt.json"],
                env=environment,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("requires fnm to activate Node", result.stderr)

    @staticmethod
    def _environment(temporary: Path, *, with_fnm: bool) -> tuple[dict[str, str], Path]:
        binary = temporary / "bin"
        binary.mkdir()
        log = temporary / "calls.log"
        node = binary / "node"
        node.write_text(
            "#!/bin/sh\n"
            "if [ \"${1:-}\" = --version ]; then\n"
            "  printf '%s\\n' \"${FAKE_NODE_VERSION:-missing}\"\n"
            "else\n"
            "  printf 'node:%s:%s\\n' \"${FAKE_NODE_VERSION:-missing}\" \"$*\" >> \"$FAKE_CALL_LOG\"\n"
            "fi\n",
            encoding="utf-8",
        )
        node.chmod(0o755)
        if with_fnm:
            fnm = binary / "fnm"
            fnm.write_text(
                "#!/bin/sh\n"
                "printf 'fnm:%s\\n' \"$*\" >> \"$FAKE_CALL_LOG\"\n"
                "[ \"$1\" = exec ] && [ \"$2\" = --using ]\n"
                "version=$3\n"
                "shift 3\n"
                "[ \"$1\" = -- ] && shift\n"
                "FAKE_NODE_VERSION=v$version exec \"$@\"\n",
                encoding="utf-8",
            )
            fnm.chmod(0o755)
        environment = {
            **os.environ,
            "PATH": f"{binary}:/usr/bin:/bin",
            "FAKE_CALL_LOG": str(log),
        }
        return environment, log


if __name__ == "__main__":
    unittest.main()
