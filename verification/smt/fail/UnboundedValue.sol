// verification/smt/fail: prove the gate rejects fixed-window arithmetic that omits its requested-amount bound.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract UnboundedValue {
    function consumeWindow(uint256 consumed, uint256 requested, uint256 limit) external pure returns (uint256 result) {
        require(consumed <= limit);
        result = consumed + requested;
        assert(result <= limit);
    }
}
