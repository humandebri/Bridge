#!/usr/bin/env python3
"""Regression tests for exact trusted proof-source profile selection."""

from __future__ import annotations

import hashlib
from pathlib import Path
import tempfile
import unittest

from trusted_proof_profiles import (
    TrustedProofProfile,
    matching_profiles,
    parse_policy,
)


class TrustedProofProfileTests(unittest.TestCase):
    def profile(self, identifier: str, path: str, content: bytes) -> TrustedProofProfile:
        return TrustedProofProfile(
            identifier,
            "current" if identifier == "current-main" else "hardening",
            {path: hashlib.sha256(content).hexdigest()},
        )

    def test_policy_requires_exact_profile_catalog(self) -> None:
        policy = "\n".join(
            (
                "schema\t1\t-\t-",
                "profile\tcurrent-main\tcurrent\t-",
                "source\tcurrent-main\tverification/claims.tsv\t" + "0" * 64,
                "profile\tsecurity-hardening-v1\thardening\t-",
                "source\tsecurity-hardening-v1\tverification/claims.tsv\t" + "1" * 64,
            )
        )
        self.assertEqual(set(parse_policy(policy)), {"current-main", "security-hardening-v1"})

    def test_selects_each_complete_profile(self) -> None:
        for identifier, content in (("current-main", b"current"), ("security-hardening-v1", b"hardening")):
            with self.subTest(identifier=identifier), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                path = root / "verification/claims.tsv"
                path.parent.mkdir(parents=True)
                path.write_bytes(content)
                profiles = {
                    identifier: self.profile(identifier, "verification/claims.tsv", content),
                }
                self.assertEqual([profile.identifier for profile in matching_profiles(root, profiles)], [identifier])

    def test_rejects_digest_change_missing_and_extra_sources(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / "verification/claims.tsv"
            path.parent.mkdir(parents=True)
            path.write_bytes(b"trusted")
            profile = self.profile("current-main", "verification/claims.tsv", b"trusted")
            profiles = {profile.identifier: profile}
            path.write_bytes(b"changed")
            self.assertEqual(matching_profiles(root, profiles), [])
            path.unlink()
            self.assertEqual(matching_profiles(root, profiles), [])
            path.write_bytes(b"trusted")
            extra = root / "verification/verus/pass.rs"
            extra.parent.mkdir(parents=True)
            extra.write_bytes(b"extra")
            self.assertEqual(matching_profiles(root, profiles), [])

    def test_rejects_mixed_profile_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            claims = root / "verification/claims.tsv"
            proof_impact = root / "verification/proof-impact.tsv"
            claims.parent.mkdir(parents=True)
            claims.write_bytes(b"current-claims")
            proof_impact.write_bytes(b"hardening-impact")
            profiles = {
                "current-main": TrustedProofProfile(
                    "current-main",
                    "current",
                    {
                        "verification/claims.tsv": hashlib.sha256(b"current-claims").hexdigest(),
                        "verification/proof-impact.tsv": hashlib.sha256(b"current-impact").hexdigest(),
                    },
                ),
                "security-hardening-v1": TrustedProofProfile(
                    "security-hardening-v1",
                    "hardening",
                    {
                        "verification/claims.tsv": hashlib.sha256(b"hardening-claims").hexdigest(),
                        "verification/proof-impact.tsv": hashlib.sha256(b"hardening-impact").hexdigest(),
                    },
                ),
            }
            self.assertEqual(matching_profiles(root, profiles), [])

    def test_rejects_untrusted_lean_manifest_vector_and_smt_config_changes(self) -> None:
        sources = {
            "verification/lean/BridgeSpec/Theorems.lean": b"theorem",
            "verification/claim-test-manifest.tsv": b"claims",
            "verification/generated/protocol-vectors.json": b"{}",
            "verification/smt/foundry.toml": b"[profile.default]",
        }
        for changed_path in sources:
            with self.subTest(path=changed_path), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                for relative, content in sources.items():
                    path = root / relative
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(content)
                profile = TrustedProofProfile(
                    "current-main",
                    "current",
                    {
                        relative: hashlib.sha256(content).hexdigest()
                        for relative, content in sources.items()
                    },
                )
                (root / changed_path).write_bytes(b"changed")
                self.assertEqual(matching_profiles(root, {profile.identifier: profile}), [])

    def test_ignores_only_generated_proof_outputs_and_caches(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            trusted = root / "verification/claims.tsv"
            trusted.parent.mkdir(parents=True)
            trusted.write_bytes(b"trusted")
            for relative in (
                "verification/output/receipt.json",
                "verification/lean/.lake/cache.bin",
                "verification/smt/out/A.json",
                "verification/smt/cache/A.json",
                "verification/halmos/.venv/stamp.json",
                "verification/certora/.venv/stamp.json",
                "verification/.DS_Store",
            ):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b"generated")
            profile = self.profile("current-main", "verification/claims.tsv", b"trusted")
            self.assertEqual(
                [item.identifier for item in matching_profiles(root, {profile.identifier: profile})],
                ["current-main"],
            )

    def test_rejects_hidden_verification_source_and_nested_halmos_harness(self) -> None:
        for relative in (
            "verification/smt/.decoy.sol",
            "verification/certora/.decoy.spec",
            "contracts/test/halmos/nested/Decoy.sol",
        ):
            with self.subTest(path=relative), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                claims = root / "verification/claims.tsv"
                claims.parent.mkdir(parents=True)
                claims.write_bytes(b"trusted")
                profile = self.profile(
                    "current-main", "verification/claims.tsv", b"trusted"
                )
                extra = root / relative
                extra.parent.mkdir(parents=True, exist_ok=True)
                extra.write_bytes(b"untrusted")
                self.assertEqual(matching_profiles(root, {profile.identifier: profile}), [])


if __name__ == "__main__":
    unittest.main()
