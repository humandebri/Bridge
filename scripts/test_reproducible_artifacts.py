#!/usr/bin/env python3

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

import check_reproducible_artifacts as reproducible


class ReproducibleArtifactTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.bundle = self.root / "bundle"
        self.first = self.root / "first"
        self.second = self.root / "second"
        for directory in (self.bundle, self.first, self.second):
            directory.mkdir()
        artifacts = []
        for name, value in (("bridge-canister.wasm", b"wasm"), ("bridge-runtime.bin", b"runtime")):
            for directory in (self.bundle, self.first, self.second):
                (directory / name).write_bytes(value)
            artifacts.append({"path": name, "sha256": hashlib.sha256(value).hexdigest()})
        (self.bundle / "release-manifest.json").write_text(
            json.dumps({"artifacts": artifacts}), encoding="utf-8"
        )

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_identical_independent_builds_are_accepted(self) -> None:
        reproducible.verify(self.bundle, self.first, self.second)

    def test_second_build_drift_is_rejected(self) -> None:
        (self.second / "bridge-canister.wasm").write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "independent builds differ"):
            reproducible.verify(self.bundle, self.first, self.second)

    def test_manifest_substitution_is_rejected(self) -> None:
        manifest = json.loads((self.bundle / "release-manifest.json").read_text())
        manifest["artifacts"][0]["sha256"] = "0" * 64
        (self.bundle / "release-manifest.json").write_text(json.dumps(manifest))
        with self.assertRaisesRegex(ValueError, "differs from release manifest"):
            reproducible.verify(self.bundle, self.first, self.second)

    def test_source_mutation_during_first_build_is_rejected(self) -> None:
        repository = self.root / "source"
        scripts = repository / "scripts"
        scripts.mkdir(parents=True)
        shutil.copy2(
            Path(__file__).with_name("rebuild-release-artifacts.sh"),
            scripts / "rebuild-release-artifacts.sh",
        )
        shutil.copy2(
            Path(__file__).with_name("check_reproducible_artifacts.py"),
            scripts / "check_reproducible_artifacts.py",
        )
        (repository / "tracked.txt").write_text("clean\n", encoding="utf-8")
        subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
        subprocess.run(["git", "config", "user.email", "proof@example.invalid"], cwd=repository, check=True)
        subprocess.run(["git", "config", "user.name", "Proof Test"], cwd=repository, check=True)
        subprocess.run(["git", "add", "."], cwd=repository, check=True)
        subprocess.run(["git", "commit", "-qm", "fixture"], cwd=repository, check=True)
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repository, text=True).strip()
        archive = subprocess.check_output(["git", "archive", "HEAD"], cwd=repository)
        tree = hashlib.sha256(archive).hexdigest()

        bundle = self.root / "release-bundle"
        bundle.mkdir()
        (bundle / "release-manifest.json").write_text('{"artifacts":[]}', encoding="utf-8")
        tools = self.root / "bin"
        tools.mkdir()
        icp = tools / "icp"
        icp.write_text(
            "#!/usr/bin/env bash\n"
            "printf 'changed\\n' > \"$(dirname \"$0\")/../source/tracked.txt\"\n"
            "mkdir -p \"$CARGO_TARGET_DIR/wasm32-unknown-unknown/release\"\n"
            "printf wasm > \"$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/bridge_canister.wasm\"\n",
            encoding="utf-8",
        )
        icp.chmod(0o755)
        environment = os.environ.copy()
        environment["PATH"] = f"{tools}:{environment['PATH']}"
        result = subprocess.run(
            ["bash", str(scripts / "rebuild-release-artifacts.sh"), str(bundle), revision, tree],
            cwd=repository,
            env=environment,
            capture_output=True,
            text=True,
            check=False,
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("clean source tree", result.stderr)


if __name__ == "__main__":
    unittest.main()
