#!/usr/bin/env python3

from pathlib import Path
import shutil
import tempfile
import unittest

from prepare_trusted_dependencies import POLICY, materialize, parse_policy
from trusted_proof_profiles import POLICY as PROOF_POLICY
from trusted_proof_profiles import parse_policy as parse_proof_policy


ROOT = Path(__file__).resolve().parents[1]


class TrustedDependencyProfileTests(unittest.TestCase):
    def test_hardening_profile_materializes_exact_manifests(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "dependencies"
            self.assertEqual(materialize(ROOT, destination), "security-hardening-v1")
            self.assertEqual(
                {
                    path.relative_to(destination).as_posix()
                    for path in destination.rglob("*")
                    if path.is_file()
                },
                set(
                    parse_policy(POLICY.read_text(encoding="utf-8"))[
                        "security-hardening-v1"
                    ]
                ),
            )

    def test_changed_manifest_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / "source"
            destination = Path(temporary) / "dependencies"
            proof_sources = parse_proof_policy(
                PROOF_POLICY.read_text(encoding="utf-8")
            )["security-hardening-v1"].sources
            dependency_sources = parse_policy(POLICY.read_text(encoding="utf-8"))[
                "security-hardening-v1"
            ]
            for relative in sorted(set(proof_sources) | set(dependency_sources)):
                target = source / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / relative, target)
            package = source / "package.json"
            package.write_text(package.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "digest mismatch"):
                materialize(source, destination)

    def test_existing_destination_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "dependencies"
            destination.mkdir()
            with self.assertRaisesRegex(ValueError, "already exists"):
                materialize(ROOT, destination)


if __name__ == "__main__":
    unittest.main()
