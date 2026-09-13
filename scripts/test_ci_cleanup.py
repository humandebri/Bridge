#!/usr/bin/env python3
"""Execute the CI driver's cleanup functions against disposable state and stubbed services."""
from pathlib import Path
import os
import re
import subprocess
import tempfile
import unittest

SOURCE = (Path(__file__).resolve().parent / "ci-local.sh").read_text()
FUNCTIONS = "\n".join(re.search(rf"^{name}\(\) \{{\n.*?^\}}", SOURCE, re.M | re.S).group()
                      for name in ("cleanup_runtime", "cleanup", "restore_smoke_canister_state"))


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        for directory in ("tmp", ".icp/cache/mappings", ".icp/cache/networks/local"):
            (self.root / directory).mkdir(parents=True)
        self.write("tmp/local.ids.json.original", "original mapping")
        self.write(".icp/cache/mappings/local.ids.json", "temporary mapping")
        self.write("tmp/icp.yaml.original", "original config")
        self.write("tmp/icp.yaml.applied", "applied config")
        self.write("icp.yaml", "applied config")
        self.write(".icp/cache/networks/local/bridge-ci-project-marker.json", "owned")

    def write(self, name, text):
        (self.root / name).write_text(text)

    def run_cleanup(self, setup="", fail="", status=0):
        script = '''set -uo pipefail
TMP_ROOT="$ROOT/tmp"
ANVIL_PID=""
ICP_NETWORK_OWNED=0
ICP_CONFIG_BACKED_UP=1
ICP_TEST_CANISTER_CREATED=0
ICP_TEST_CANISTER_SNAPSHOT="snapshot-1"
ICP_TEST_CANISTER_WAS_RUNNING=1
ICP_LOCAL_MAPPING_BACKED_UP=1
ICP_SMOKE_STATE_PREPARED=1
CLEANUP_DONE=0
icp() {
  printf '%s\\n' "$*" >> "$ROOT/calls"
  [[ -z "$FAIL" || "$*" != "$FAIL"* ]]
}
kill() { printf 'kill %s\\n' "$*" >> "$ROOT/calls"; }
wait() { printf 'wait %s\\n' "$*" >> "$ROOT/calls"; }
''' + FUNCTIONS + "\n" + setup + '\ntrap cleanup EXIT\nexit ' + str(status)
        result = subprocess.run(["bash", "-c", script], env={**os.environ, "ROOT": str(self.root), "FAIL": fail}, capture_output=True, text=True)
        calls = (self.root / "calls").read_text() if (self.root / "calls").exists() else ""
        return result.returncode, calls

    def test_restore_failure_retains_snapshot_and_artifacts_but_restores_mapping_and_config(self):
        code, calls = self.run_cleanup(fail="canister snapshot restore")
        self.assertEqual(code, 1)
        self.assertNotIn("canister snapshot delete", calls)
        self.assertNotIn("canister start", calls)
        self.assertTrue((self.root / "tmp").is_dir())
        self.assertEqual((self.root / ".icp/cache/mappings/local.ids.json").read_text(), "original mapping")
        self.assertEqual((self.root / "icp.yaml").read_text(), "original config")

    def test_success_restores_then_restarts_and_waits_only_for_owned_anvil(self):
        code, calls = self.run_cleanup('ANVIL_PID=12345')
        self.assertEqual(code, 0)
        self.assertLess(calls.index("canister snapshot restore"), calls.index("canister start"))
        self.assertIn("kill -0 12345\nkill 12345\nwait 12345\n", calls)
        self.assertFalse((self.root / "tmp").exists())

    def test_temporary_canister_is_deleted_and_external_config_edit_is_preserved(self):
        self.write("icp.yaml", "external edit")
        code, calls = self.run_cleanup("ICP_TEST_CANISTER_CREATED=1")
        self.assertEqual(code, 0)
        self.assertIn("canister delete", calls)
        self.assertNotIn("snapshot restore", calls)
        self.assertNotIn("canister start", calls)
        self.assertEqual((self.root / "icp.yaml").read_text(), "external edit")

    def test_failed_owned_network_stop_preserves_marker_and_original_exit(self):
        code, calls = self.run_cleanup("ICP_NETWORK_OWNED=1", fail="network stop", status=23)
        self.assertEqual(code, 23)
        self.assertNotIn("canister", calls)
        self.assertTrue((self.root / ".icp/cache/networks/local/bridge-ci-project-marker.json").exists())
        self.assertTrue((self.root / "tmp").exists())


if __name__ == "__main__":
    unittest.main()
