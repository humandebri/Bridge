#!/usr/bin/env python3
"""Unit tests for fail-closed Certora cloud result validation."""

from __future__ import annotations

import json
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
    @staticmethod
    def progress(*rules: object) -> dict[str, str]:
        return {"verificationProgress": json.dumps({"rules": list(rules)})}

    @staticmethod
    def node(
        name: str, status: object = "VERIFIED", children: object = None
    ) -> dict[str, object]:
        return {
            "name": name,
            "status": status,
            "children": [] if children is None else children,
        }

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

    def test_sanity_failures_warnings_and_unknown_statuses_are_rejected(self) -> None:
        for status in (
            False,
            None,
            1,
            "",
            "SANITY_FAILED",
            "SANITY_FAIL",
            "WARNING",
            "TIMEOUT",
            "UNKNOWN",
            "FALSE",
            "NEW_SUCCESS_STATUS",
        ):
            with self.subTest(status=status):
                with self.assertRaisesRegex(CertoraResultError, "sanity result"):
                    validate_sanity(self.progress(self.node("rule sanity", status)))

    def test_only_explicit_passing_sanity_statuses_are_accepted(self) -> None:
        for status in (True, "VERIFIED", "SUCCESS", "PASS", "PASSED", " passed "):
            with self.subTest(status=status):
                validate_sanity(self.progress(self.node("rule sanity", status)))

    def test_sanity_evidence_must_be_present_and_non_vacuous(self) -> None:
        with self.assertRaisesRegex(CertoraResultError, "no positive sanity evidence"):
            validate_sanity(self.progress(self.node("ordinary rule")))
        for name in ("vacuity check", "trivial sanity"):
            with self.subTest(name=name):
                with self.assertRaisesRegex(CertoraResultError, "vacuous or trivial"):
                    validate_sanity(self.progress(self.node(name)))

    def test_progress_envelope_and_node_schema_are_fail_closed(self) -> None:
        invalid = (
            None,
            {},
            {"verificationProgress": None},
            {"verificationProgress": ""},
            {"verificationProgress": "{"},
            {"verificationProgress": "null"},
            {"verificationProgress": json.dumps({"rules": []})},
            {
                "verificationProgress": json.dumps(
                    {"rules": [self.node("rule sanity")], "extra": True}
                )
            },
            {
                "verificationProgress": json.dumps(
                    {"rules": [{"name": "rule sanity", "status": "VERIFIED"}]}
                )
            },
            {
                "verificationProgress": json.dumps(
                    {"rules": [{"name": "rule sanity", "children": []}]}
                )
            },
        )
        for progress in invalid:
            with self.subTest(progress=progress):
                with self.assertRaises(CertoraResultError):
                    validate_sanity(progress)

    def test_nested_sanity_node_is_required_and_accepted(self) -> None:
        validate_sanity(
            self.progress(
                self.node(
                    "bridge rule",
                    children=[self.node("bridge rule sanity", "VERIFIED")],
                )
            )
        )

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
