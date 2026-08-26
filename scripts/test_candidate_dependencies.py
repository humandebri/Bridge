#!/usr/bin/env python3

from pathlib import Path
import shutil
import tempfile
import unittest

from prepare_candidate_dependencies import (
    REQUIRED_MANIFESTS,
    candidate_dependency_sources,
    materialize,
)


ROOT = Path(__file__).resolve().parents[1]


class CandidateDependencyTests(unittest.TestCase):
    def test_materializes_exact_fixed_manifests_and_patches(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "dependencies"
            copied = materialize(ROOT, destination)
            self.assertEqual(
                {
                    path.relative_to(destination).as_posix()
                    for path in destination.rglob("*")
                    if path.is_file()
                },
                set(copied),
            )
            self.assertTrue(set(REQUIRED_MANIFESTS).issubset(copied))
            self.assertTrue(any(path.endswith(".patch") for path in copied))

    def test_candidate_manifest_content_is_not_static_profile_locked(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            package = source / "package.json"
            package.write_text(package.read_text(encoding="utf-8") + "\n", encoding="utf-8")
            destination = Path(temporary) / "dependencies"
            materialize(source, destination)
            self.assertEqual(destination.joinpath("package.json").read_bytes(), package.read_bytes())

    def test_symlinked_input_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            package = source / "package.json"
            package.unlink()
            package.symlink_to(source / "pnpm-workspace.yaml")
            with self.assertRaisesRegex(ValueError, "not regular"):
                candidate_dependency_sources(source)

    def test_unknown_manifest_placement_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            extra = source / "nested" / "package.json"
            extra.parent.mkdir()
            extra.write_text("{}\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "unknown dependency manifest placement"):
                candidate_dependency_sources(source)

    def test_unknown_patch_input_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            (source / "ui" / "patches" / "README.md").write_text("no\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "unknown candidate patch input"):
                candidate_dependency_sources(source)

    def test_existing_destination_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "dependencies"
            destination.mkdir()
            with self.assertRaisesRegex(ValueError, "already exists"):
                materialize(ROOT, destination)

    @staticmethod
    def _copy_inputs(temporary: Path) -> Path:
        source = temporary / "source"
        for relative in REQUIRED_MANIFESTS:
            destination = source / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        shutil.copytree(ROOT / "ui" / "patches", source / "ui" / "patches")
        return source


if __name__ == "__main__":
    unittest.main()
