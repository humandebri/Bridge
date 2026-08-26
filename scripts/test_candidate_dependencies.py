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

    def test_missing_required_manifest_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            (source / "ui" / "pnpm-lock.yaml").unlink()
            with self.assertRaisesRegex(
                ValueError, "required dependency manifests are missing"
            ):
                candidate_dependency_sources(source)

    def test_unknown_manifest_placement_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            extra = source / "nested" / "package.json"
            extra.parent.mkdir()
            extra.write_text("{}\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "unknown dependency manifest placement"):
                candidate_dependency_sources(source)

    def test_materialized_node_modules_are_not_candidate_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            installed = source / "node_modules" / "dependency" / "package.json"
            installed.parent.mkdir(parents=True)
            installed.write_text("{}\n", encoding="utf-8")
            self.assertNotIn(
                installed.relative_to(source).as_posix(),
                candidate_dependency_sources(source),
            )

    def test_certora_generated_manifests_are_not_candidate_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            generated = (
                source
                / ".certora_internal"
                / "run"
                / ".certora_sources"
                / "package.json"
            )
            generated.parent.mkdir(parents=True)
            generated.write_text("{}\n", encoding="utf-8")
            self.assertNotIn(
                generated.relative_to(source).as_posix(),
                candidate_dependency_sources(source),
            )

    def test_submodule_manifests_are_not_workspace_manifest_placements(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            nested = source / "vendor" / "submodule"
            nested.mkdir(parents=True)
            (nested / ".git").write_text("gitdir: fixture\n", encoding="utf-8")
            package = nested / "package.json"
            package.write_text("{}\n", encoding="utf-8")
            self.assertNotIn(
                package.relative_to(source).as_posix(),
                candidate_dependency_sources(source),
            )

    def test_unknown_patch_input_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            (source / "ui" / "patches" / "README.md").write_text("no\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "unknown candidate patch input"):
                candidate_dependency_sources(source)

    def test_absent_patch_directory_is_an_empty_patch_set(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            shutil.rmtree(source / "ui" / "patches")
            self.assertEqual(
                set(candidate_dependency_sources(source)), set(REQUIRED_MANIFESTS)
            )

    def test_symlinked_patch_file_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            source = self._copy_inputs(Path(temporary))
            patch = next((source / "ui" / "patches").glob("*.patch"))
            patch.unlink()
            patch.symlink_to(source / "package.json")
            with self.assertRaisesRegex(ValueError, "not regular"):
                candidate_dependency_sources(source)

    def test_existing_destination_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "dependencies"
            destination.mkdir()
            with self.assertRaisesRegex(ValueError, "already exists"):
                materialize(ROOT, destination)

    def test_broken_symlink_destination_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            destination = Path(temporary) / "dependencies"
            destination.symlink_to(Path(temporary) / "missing")
            with self.assertRaisesRegex(ValueError, "already exists"):
                materialize(ROOT, destination)

    def test_symlinked_destination_parent_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            actual = root / "actual"
            actual.mkdir()
            linked = root / "linked"
            linked.symlink_to(actual, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, "symlinked parent"):
                materialize(ROOT, linked / "dependencies")

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
