#!/usr/bin/env python3
"""Regression tests for the fail-closed Base Sepolia staging upgrade driver."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "plan007" / "staging-canister-upgrade.sh"
CHAIN_ID = 84532
OLD_DIGEST = "e9b9c716dedf57245c75b8d87114b065a55a96bd0f7bd56691683722ac5721fb"
NEW_DIGEST = "3ab53c0532b80b3f39ed076f9661794c0a847b0d2eba1845b5c7e0ed1663ed48"
OLD_URLS = [
    "https://base-sepolia-rpc.publicnode.com",
    "https://base-sepolia.gateway.tenderly.co",
    "https://sepolia.base.org",
]
NEW_URLS = [
    "https://base-sepolia-rpc.publicnode.com",
    "https://sepolia.base.org",
    "https://base-sepolia.api.onfinality.io/public",
]


def rpc_digest(urls: list[str]) -> str:
    encoded = json.dumps(urls, separators=(",", ":")).encode()
    return hashlib.sha256(encoded).hexdigest()


class StagingCanisterUpgradeTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary_directory = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary_directory.name)
        script_directory = self.root / "scripts" / "plan007"
        script_directory.mkdir(parents=True)
        shutil.copy2(SCRIPT, script_directory / SCRIPT.name)
        canister_directory = self.root / "canister" / "bridge-canister"
        canister_directory.mkdir(parents=True)
        (canister_directory / "bridge.did").write_text("service : {}\n", encoding="utf-8")
        (self.root / "deployments" / "sepolia-staging").mkdir(parents=True)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.cast_state = self.root / "cast-state"
        self.icp_state = self.root / "icp-state"
        self.deploy_record = self.root / "deploy.json"
        self.write_executable(
            "cast",
            """#!/usr/bin/env python3
import os
from pathlib import Path
import sys

state = Path(os.environ["MOCK_CAST_STATE"])
count = int(state.read_text() or "0") if state.exists() else 0
state.write_text(str(count + 1))
if os.environ.get("MOCK_CAST_FAIL_AT") == str(count):
    raise SystemExit(1)
print(os.environ.get("MOCK_WRONG_CHAIN_AT") == str(count) and "1" or "84532")
""",
        )
        self.write_executable(
            "icp",
            """#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

args = sys.argv[1:]
if args and args[0] == "deploy":
    Path(os.environ["MOCK_DEPLOY_RECORD"]).write_text(json.dumps(args))
    if os.environ.get("MOCK_DEPLOY_FAIL") == "1":
        raise SystemExit(1)
    raise SystemExit(0)
if args[:2] != ["canister", "call"]:
    raise SystemExit(2)
state = Path(os.environ["MOCK_ICP_STATE"])
count = int(state.read_text() or "0") if state.exists() else 0
state.write_text(str(count + 1))
digests = os.environ["MOCK_LIVE_DIGESTS"].split(",")
digest = digests[min(count, len(digests) - 1)]
blob = "".join("\\\\" + digest[index:index + 2] for index in range(0, len(digest), 2))
print(json.dumps({"response_candid": f'record {{ rpc_provider_urls_sha256 = blob "{blob}" }}'}))
""",
        )
        self.write_profile(NEW_URLS)

    def tearDown(self) -> None:
        self.temporary_directory.cleanup()

    def write_executable(self, name: str, source: str) -> None:
        path = self.bin / name
        path.write_text(source, encoding="utf-8")
        path.chmod(0o755)

    def write_profile(
        self,
        urls: list[str],
        *,
        chain_id: int = CHAIN_ID,
        digest: str | None = None,
        environment: str = "sepolia-staging",
    ) -> None:
        value = {
            "environment": environment,
            "chainId": chain_id,
            "baseRpcUrl": urls[0],
            "baseHistoryRpcUrls": urls[1:],
            "rpcProviderUrlsSha256": f"0x{digest or rpc_digest(urls)}",
        }
        profile = self.root / "deployments" / "sepolia-staging" / "frontend-profile.json"
        profile.write_text(json.dumps(value), encoding="utf-8")

    def run_driver(
        self,
        *arguments: str,
        identity: str | None = "reviewed-staging-operator",
        live_digests: tuple[str, ...] = (OLD_DIGEST, NEW_DIGEST),
        environment: dict[str, str] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env.update(
            {
                "PATH": f"{self.bin}{os.pathsep}{env['PATH']}",
                "MOCK_CAST_STATE": str(self.cast_state),
                "MOCK_ICP_STATE": str(self.icp_state),
                "MOCK_DEPLOY_RECORD": str(self.deploy_record),
                "MOCK_LIVE_DIGESTS": ",".join(live_digests),
            }
        )
        if identity is None:
            env.pop("BRIDGE_STAGING_IDENTITY", None)
        else:
            env["BRIDGE_STAGING_IDENTITY"] = identity
        if environment:
            env.update(environment)
        return subprocess.run(
            ["bash", str(self.root / "scripts" / "plan007" / SCRIPT.name), *arguments],
            text=True,
            capture_output=True,
            check=False,
            env=env,
        )

    def test_reviewed_url_sets_have_the_expected_digests(self) -> None:
        self.assertEqual(rpc_digest(NEW_URLS), NEW_DIGEST)

    def test_requires_execute_identity_and_tools(self) -> None:
        self.assertEqual(self.run_driver().returncode, 2)
        self.assertEqual(self.run_driver("--execute", identity=None).returncode, 2)
        (self.bin / "cast").unlink()
        result = self.run_driver(
            "--execute", environment={"PATH": f"{self.bin}:/usr/bin:/bin"}
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cast is required", result.stderr)

    def test_rejects_unreviewed_profile_binding_before_live_calls(self) -> None:
        self.write_profile(OLD_URLS, digest=OLD_DIGEST)
        result = self.run_driver("--execute")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not bind its configured URLs", result.stderr)
        self.assertFalse(self.icp_state.exists())

        self.write_profile(NEW_URLS, digest="0" * 64)
        result = self.run_driver("--execute")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("does not bind its configured URLs", result.stderr)

    def test_rejects_rpc_failure_or_wrong_chain_before_live_calls(self) -> None:
        for variable in ("MOCK_CAST_FAIL_AT", "MOCK_WRONG_CHAIN_AT"):
            with self.subTest(variable=variable):
                self.cast_state.unlink(missing_ok=True)
                result = self.run_driver("--execute", environment={variable: "1"})
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.icp_state.exists())

    def test_rejects_an_unreviewed_live_digest(self) -> None:
        result = self.run_driver("--execute", live_digests=("1" * 64,))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("refusing upgrade from unreviewed live RPC digest", result.stderr)
        self.assertFalse(self.deploy_record.exists())

    def test_upgrades_old_digest_and_verifies_the_new_digest(self) -> None:
        result = self.run_driver("--execute")
        self.assertEqual(result.returncode, 0, result.stderr)
        deploy = json.loads(self.deploy_record.read_text(encoding="utf-8"))
        self.assertIn("--mode", deploy)
        self.assertEqual(deploy[deploy.index("--mode") + 1], "upgrade")
        candid = deploy[deploy.index("--args") + 1]
        for url in NEW_URLS:
            self.assertIn(json.dumps(url), candid)
        self.assertIn(f"RPC digest {NEW_DIGEST}", result.stdout)

    def test_already_applied_digest_skips_deploy_after_chain_checks(self) -> None:
        result = self.run_driver("--execute", live_digests=(NEW_DIGEST,))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.cast_state.read_text(encoding="utf-8"), "3")
        self.assertFalse(self.deploy_record.exists())
        self.assertIn("already uses the reviewed RPC digest", result.stdout)

    def test_deploy_failure_and_post_upgrade_mismatch_fail_closed(self) -> None:
        failed = self.run_driver("--execute", environment={"MOCK_DEPLOY_FAIL": "1"})
        self.assertNotEqual(failed.returncode, 0)

        self.icp_state.unlink(missing_ok=True)
        self.cast_state.unlink(missing_ok=True)
        self.deploy_record.unlink(missing_ok=True)
        mismatched = self.run_driver(
            "--execute", live_digests=(OLD_DIGEST, OLD_DIGEST)
        )
        self.assertNotEqual(mismatched.returncode, 0)
        self.assertIn("without activating the reviewed OnFinality RPC digest", mismatched.stderr)


if __name__ == "__main__":
    unittest.main()
