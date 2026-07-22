#!/usr/bin/env python3
"""Regression tests for protocol vector drift detection."""

from __future__ import annotations

import contextlib
import io
import unittest

import protocol_vectors


class ProtocolVectorDriftTests(unittest.TestCase):
    def test_identical_vector_is_accepted(self) -> None:
        self.assertTrue(protocol_vectors.matches_expected('{"schema_version":1}\n', '{"schema_version":1}\n', "fixture"))

    def test_changed_vector_is_rejected(self) -> None:
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            accepted = protocol_vectors.matches_expected(
                '{"schema_version":1}\n',
                '{"schema_version":2}\n',
                "fixture",
            )
        self.assertFalse(accepted)
        self.assertIn("-{'schema_version':1}".replace("'", '"'), stderr.getvalue())
        self.assertIn("+{'schema_version':2}".replace("'", '"'), stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
