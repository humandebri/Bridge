#!/usr/bin/env python3
"""Semantic review drift and trusted review boundary regressions."""
import copy
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import check_claim_semantics as semantics
from ci_changed_areas import review_required


class SemanticsTests(unittest.TestCase):
    def setUp(self):
        self.snapshot = {
            "declarations": [
                {"name": "BridgeSpec.Contract", "type": "Prop", "definition": "A → B ∧ C"},
                {"name": "BridgeSpec.witness", "type": "BridgeSpec.Contract", "definition": None},
                {"name": "BridgeSpec.guard", "type": "Bool → Bool", "definition": "fun x => x"}],
            "sources": {"Model.lean": {"sha256": "old", "lines": 1, "source_declarations": 1}},
            "roots": {"claim": {"contract": "BridgeSpec.Contract", "witness": "BridgeSpec.witness"}},
            "semantics": [{"id": "claim", "definitions": "BridgeSpec.Contract;BridgeSpec.guard",
                           "review_note": "reviewed"}],
        }

    def test_conclusion_removed_requires_correspondence_update(self):
        changed = copy.deepcopy(self.snapshot)
        changed["declarations"][0]["definition"] = "A → B"
        result = semantics.compare_snapshots(self.snapshot, changed)
        self.assertEqual(result["missing_semantics_updates"], ["claim"])
        self.assertTrue(result["review_required"])

    def test_additional_premise_requires_correspondence_update(self):
        changed = copy.deepcopy(self.snapshot)
        changed["declarations"][0]["definition"] = "A ∧ D → B ∧ C"
        self.assertEqual(semantics.compare_snapshots(self.snapshot, changed)["missing_semantics_updates"], ["claim"])

    def test_definition_only_change_is_detected(self):
        changed = copy.deepcopy(self.snapshot)
        changed["declarations"][2]["definition"] = "fun _ => true"
        self.assertEqual(semantics.compare_snapshots(self.snapshot, changed)["missing_semantics_updates"], ["claim"])

    def test_updated_note_never_suppresses_head_review(self):
        changed = copy.deepcopy(self.snapshot)
        changed["declarations"][0]["definition"] = "A → B"
        changed["semantics"][0]["review_note"] = "Explain changed guarantee"
        result = semantics.compare_snapshots(self.snapshot, changed)
        self.assertEqual(result["missing_semantics_updates"], [])
        self.assertTrue(result["review_required"])

    def test_proof_or_comment_change_is_conservatively_reported(self):
        changed = copy.deepcopy(self.snapshot)
        changed["sources"]["Model.lean"]["sha256"] = "new"
        result = semantics.compare_snapshots(self.snapshot, changed)
        self.assertEqual(result["changed_sources"], ["Model.lean"])
        self.assertEqual(result["changed_declarations"], [])
        self.assertEqual(result["missing_semantics_updates"], [])
        self.assertTrue(result["review_required"])

    def test_all_snapshot_and_registry_changes_require_trusted_review(self):
        for path in (semantics.SNAPSHOT, semantics.MARKDOWN, semantics.SEMANTICS,
                     semantics.DEFINITIONS, "verification/lean/BridgeSpec/Model.lean"):
            self.assertTrue(review_required([path]), path)
        self.assertTrue(review_required([semantics.SNAPSHOT, semantics.SEMANTICS]))

    def test_unchanged_snapshot_does_not_claim_semantic_changes(self):
        self.assertFalse(semantics.compare_snapshots(self.snapshot, self.snapshot)["review_required"])

    def test_invalid_lean_name_cannot_inject_code(self):
        with self.assertRaises(ValueError):
            semantics.export_source(["BridgeSpec.x\naxiom exploit : False"])

    def test_export_reads_types_and_never_prints_witness_proofs(self):
        source = semantics.export_source(["BridgeSpec.witness"])
        self.assertIn("declarationJson `BridgeSpec.witness", source)
        self.assertNotIn("#print BridgeSpec.witness", source)
        exporter = (semantics.ROOT / "verification/lean/BridgeSpec/AuditExport.lean").read_text()
        self.assertNotIn(".thmInfo", exporter)
        self.assertIn(".defnInfo", exporter)

    def test_stale_artifacts_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(semantics, "build_snapshot", return_value=self.snapshot), \
                 patch.object(semantics, "render_markdown", return_value="report\n"):
                root = Path(directory)
                semantics.check_semantics(root, write=True)
                semantics.check_semantics(root)
                (root / semantics.SNAPSHOT).write_text("{}")
                with self.assertRaisesRegex(ValueError, "stale semantics"):
                    semantics.check_semantics(root)

    def test_catalog_missing_or_duplicate_row_rejected(self):
        original = semantics.read_table
        for mutation in (lambda rows: rows[:-1], lambda rows: rows + rows[:1]):
            def read(path, fields):
                rows = original(path, fields)
                return mutation(rows) if fields == semantics.FIELDS else rows
            with patch.object(semantics, "read_table", side_effect=read):
                with self.assertRaisesRegex(ValueError, "cover exactly"):
                    semantics.load_registry()

    def test_unregistered_definition_rejected(self):
        original = semantics.read_table
        def read(path, fields):
            rows = original(path, fields)
            if fields == semantics.FIELDS:
                rows[0]["definitions"] += ";BridgeSpec.missing"
            return rows
        with patch.object(semantics, "read_table", side_effect=read):
            with self.assertRaisesRegex(ValueError, "unregistered"):
                semantics.load_registry()

    def test_invalid_base_is_rejected_before_git(self):
        with self.assertRaisesRegex(ValueError, "immutable"):
            semantics.check_base(self.snapshot, "HEAD")

    def test_lean_version_is_platform_independent(self):
        apple = "Lean (version 4.30.0, arm64-apple-darwin24.6.0, commit d024af0, Release)"
        linux = "Lean (version 4.30.0, x86_64-unknown-linux-gnu, commit d024af0, Release)"
        self.assertEqual(semantics.normalized_lean_version(apple), semantics.normalized_lean_version(linux))
        with self.assertRaises(ValueError):
            semantics.normalized_lean_version("unknown")

    def test_live_registry_has_43_plus_5_entries(self):
        rows, definitions, roots = semantics.load_registry()
        self.assertEqual(sum(r["kind"] == "claim" for r in rows), 43)
        self.assertEqual(sum(r["kind"] == "liveness" for r in rows), 5)
        self.assertEqual(len(roots), 48)
        self.assertGreater(len(definitions), 48)


if __name__ == "__main__":
    unittest.main()
