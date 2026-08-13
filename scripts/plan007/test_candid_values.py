#!/usr/bin/env python3
from __future__ import annotations

import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from candid_values import blob  # noqa: E402


class CandidBlobTests(unittest.TestCase):
    VALUE = bytes.fromhex("3ab53c0532b80b3f39ed076f9661794c0a847b0d2eba1845b5c7e0ed1663ed48")

    def test_vec_and_mixed_printable_blob_decode_identically(self) -> None:
        vector = "; ".join(f"{value} : nat8" for value in self.VALUE)
        mixed = "\\3a\\b5<\\052\\b8\\0b?9\\ed\\07o\\96ayL\\0a\\84{\\0d.\\ba\\18E\\b5\\c7\\e0\\ed\\16c\\edH"
        self.assertEqual(blob(f"record {{ digest = vec {{ {vector} }} }}", "digest", length=32), self.VALUE)
        self.assertEqual(blob(f'record {{ digest = blob "{mixed}" }}', "digest", length=32), self.VALUE)

    def test_quote_backslash_and_named_escapes(self) -> None:
        self.assertEqual(blob(r'record { value = blob "a\"b\\c\n\r\t" }', "value"), b'a"b\\c\n\r\t')

    def test_rejects_duplicate_wrong_length_range_and_bad_escape(self) -> None:
        cases = [
            'record { x = blob "a"; x = blob "b" }',
            'record { x = blob "short" }',
            'record { x = vec { 256 : nat8 } }',
            r'record { x = blob "\q" }',
        ]
        for candid in cases:
            with self.subTest(candid=candid):
                with self.assertRaises(ValueError):
                    blob(candid, "x", length=32)


if __name__ == "__main__":
    unittest.main()
