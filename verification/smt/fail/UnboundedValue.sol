// verification/smt/fail: provide a deliberate counterexample proving SMTChecker is active.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract UnboundedValue {
    function accept(uint256 value) external pure returns (uint256) {
        assert(value == 0);
        return value;
    }
}
