#!/usr/bin/env python3
"""Regression tests for the fail-closed LCOV coverage policy."""

import unittest

import check_contract_coverage


def lcov(*, lines: tuple[int, int], branches: tuple[int, int], functions: tuple[int, int]) -> str:
    return "\n".join(
        (
            "TN:",
            "SF:contracts/src/Bridge.sol",
            f"FNF:{functions[1]}",
            f"FNH:{functions[0]}",
            f"BRF:{branches[1]}",
            f"BRH:{branches[0]}",
            f"LF:{lines[1]}",
            f"LH:{lines[0]}",
            "end_of_record",
            "",
        )
    )


class ContractCoverageTests(unittest.TestCase):
    def test_exact_thresholds_pass(self) -> None:
        coverage = check_contract_coverage.parse_lcov(
            lcov(lines=(76, 100), branches=(74, 100), functions=(68, 100))
        )
        check_contract_coverage.enforce_thresholds(coverage)

    def test_one_basis_point_below_each_threshold_fails(self) -> None:
        cases = {
            "lines": dict(lines=(7599, 10_000), branches=(74, 100), functions=(68, 100)),
            "branches": dict(lines=(76, 100), branches=(7399, 10_000), functions=(68, 100)),
            "functions": dict(lines=(76, 100), branches=(74, 100), functions=(6799, 10_000)),
        }
        for metric, values in cases.items():
            with self.subTest(metric=metric):
                coverage = check_contract_coverage.parse_lcov(lcov(**values))
                with self.assertRaisesRegex(ValueError, metric):
                    check_contract_coverage.enforce_thresholds(coverage)

    def test_missing_summary_is_rejected(self) -> None:
        source = lcov(lines=(76, 100), branches=(74, 100), functions=(68, 100))
        with self.assertRaisesRegex(ValueError, "FNH/FNF"):
            check_contract_coverage.parse_lcov(source.replace("FNH:68\n", ""))

    def test_zero_denominator_is_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "branches denominator is zero"):
            check_contract_coverage.parse_lcov(
                lcov(lines=(76, 100), branches=(0, 0), functions=(68, 100))
            )

    def test_malformed_and_empty_lcov_are_rejected(self) -> None:
        invalid = (
            "",
            "TN:\nSF:x.sol\nend_of_record\n",
            lcov(lines=(76, 100), branches=(74, 100), functions=(68, 100)).replace(
                "LH:76", "LH:not-an-int"
            ),
            lcov(lines=(76, 100), branches=(74, 100), functions=(68, 100)).replace(
                "end_of_record", ""
            ),
        )
        for source in invalid:
            with self.subTest(source=source):
                with self.assertRaises(ValueError):
                    check_contract_coverage.parse_lcov(source)


if __name__ == "__main__":
    unittest.main()
