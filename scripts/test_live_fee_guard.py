#!/usr/bin/env python3
import importlib.util
import pathlib
import unittest


PATH = pathlib.Path(__file__).with_name("live_fee_guard.py")
SPEC = importlib.util.spec_from_file_location("live_fee_guard", PATH)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class LiveFeeGuardTests(unittest.TestCase):
    def setUp(self):
        self.profile = {"parameters": {"ledger_fee": 100_000, "service_fee": 1_000_000}}
        self.base = {"state": {"base_service_fee": 1_000_000}}

    def test_accepts_reviewed_live_fees(self):
        self.assertEqual(
            MODULE.validate(self.profile, self.base, "100_000"),
            {"base_service_fee": 1_000_000, "ledger_fee": 100_000},
        )

    def test_rejects_ledger_fee_drift(self):
        with self.assertRaisesRegex(ValueError, "Ledger fee drift"):
            MODULE.validate(self.profile, self.base, 100_001)

    def test_rejects_base_service_fee_drift(self):
        with self.assertRaisesRegex(ValueError, "Base service fee drift"):
            MODULE.validate(self.profile, {"state": {"base_service_fee": 999_999}}, 100_000)

    def test_rejects_fee_relation_violation(self):
        profile = {"parameters": {"ledger_fee": 10, "service_fee": 9}}
        with self.assertRaisesRegex(ValueError, "exceeds"):
            MODULE.validate(profile, {"state": {"base_service_fee": 9}}, 10)


if __name__ == "__main__":
    unittest.main()
