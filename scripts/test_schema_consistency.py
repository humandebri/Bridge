"""Schema declarations remain exact after English documentation changes."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from check_schema_consistency import require_versions


class SchemaConsistencyTests(unittest.TestCase):
    def test_english_schema_and_wire_declarations_fail_closed(self):
        pattern = r"Current formats are stable schema v(\d+)"
        cases = (
            ("Current formats are stable schema v36 and record wire v30.\n" * 2, None),
            ("Current formats are stable schema v35 and record wire v30.\n" * 2, "mismatch"),
            ("Current formats are stable schema v36 and record wire v30.\n", "count mismatch"),
            ("Current formats are stable schema v36 and record wire v30.\n" * 3, "count mismatch"),
            ("No declaration", "missing"),
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "doc.md"
            for text, error in cases:
                with self.subTest(text=text), patch("check_schema_consistency.source_path", return_value=path):
                    path.write_text(text)
                    if error:
                        with self.assertRaisesRegex(SystemExit, error):
                            require_versions("doc.md", pattern, 36, 2)
                    else:
                        require_versions("doc.md", pattern, 36, 2)
                        require_versions("doc.md", r"record wire v(\d+)", 30, 2)
                        with self.assertRaisesRegex(SystemExit, "mismatch"):
                            require_versions("doc.md", r"record wire v(\d+)", 29, 2)


if __name__ == "__main__":
    unittest.main()
