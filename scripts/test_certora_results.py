#!/usr/bin/env python3
"""Unit tests for fail-closed Certora cloud result validation."""

from __future__ import annotations

import unittest
import urllib.error
from unittest import mock

from certora_results import (
    CertoraResultError,
    fetch_json,
    private_report_url,
    prover_revision,
    validate_rule_results,
    validate_sanity,
)
from certora_fingerprint import validate_certora_fingerprint


class CertoraResultTests(unittest.TestCase):
    def test_result_parser_requires_certora_fingerprint_scope(self) -> None:
        with self.assertRaisesRegex(ValueError, "shape"):
            validate_certora_fingerprint(
                {"algorithm": "sha256", "digest": "a" * 64, "input_count": 1}
            )

    def test_successful_nested_results_are_normalized(self) -> None:
        output = {
            "rules": {
                "one": "SUCCESS",
                "two": {"SUCCESS": ["method()"], "FAIL": []},
            }
        }
        self.assertEqual(
            validate_rule_results(output, {"one", "two"}),
            {"one": "SUCCESS", "two": "SUCCESS"},
        )

    def test_missing_rule_is_rejected(self) -> None:
        with self.assertRaisesRegex(CertoraResultError, "result set mismatch"):
            validate_rule_results({"rules": {"one": "SUCCESS"}}, {"one", "two"})

    def test_empty_results_are_rejected(self) -> None:
        with self.assertRaisesRegex(CertoraResultError, "no rule results"):
            validate_rule_results({"rules": {}}, {"one"})

    def test_every_failure_status_is_rejected(self) -> None:
        for status in ("FAIL", "TIMEOUT", "UNKNOWN", "SANITY_FAIL"):
            with self.subTest(status=status):
                with self.assertRaisesRegex(CertoraResultError, "did not pass"):
                    validate_rule_results({"rules": {"one": status}}, {"one"})

    def test_sanity_failures_and_warnings_are_rejected(self) -> None:
        for status in ("SANITY_FAILED", "SANITY_FAIL"):
            with self.subTest(status=status):
                with self.assertRaisesRegex(CertoraResultError, "sanity result"):
                    validate_sanity({"status": status})
        with self.assertRaisesRegex(CertoraResultError, "sanity result"):
            validate_sanity({"children": [{"sanityStatus": "WARNING: trivial rule"}]})
        with self.assertRaisesRegex(CertoraResultError, "sanity result"):
            validate_sanity({"children": [{"type": "SANITY", "severity": "WARNING"}]})

    def test_clean_or_unrelated_status_is_accepted(self) -> None:
        validate_sanity({"children": [{"type": "SANITY", "status": "VERIFIED"}]})
        validate_sanity({"status": "WARNING"})

    def test_exact_prover_revision_is_required(self) -> None:
        with self.assertRaisesRegex(CertoraResultError, "exact Prover revision"):
            prover_revision({"branch": "release/15June2026"})
        self.assertEqual(
            prover_revision({"git_hash": "a" * 40}),
            "a" * 40,
        )

    def test_artifact_report_url_omits_anonymous_key(self) -> None:
        job = {
            "domain": "https://prover.certora.com",
            "user_id": 12,
            "job_id": "abc",
            "anonymous_key": "must-not-leak",
        }
        report = private_report_url(job)
        self.assertEqual(report, "https://prover.certora.com/output/12/abc")
        self.assertNotIn("must-not-leak", report)

    def test_fetch_failure_does_not_echo_private_url(self) -> None:
        private = "https://prover.certora.com/jobData/12/abc?anonymousKey=must-not-leak"
        error = urllib.error.HTTPError(private, 500, "failure", {}, None)
        with mock.patch("urllib.request.urlopen", side_effect=error):
            with self.assertRaises(CertoraResultError) as raised:
                fetch_json(private)
        self.assertNotIn("must-not-leak", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
