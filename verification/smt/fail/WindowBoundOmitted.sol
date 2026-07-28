// verification/smt/fail: successful window consumption must remain within its limit.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract WindowBoundOmitted {
    function acceptsWindowOverflow(uint256 limit) external pure {
        require(limit < type(uint256).max);
        uint256 consumedAfter = limit + 1;
        bool accepted = true;
        assert(!accepted || consumedAfter <= limit);
    }
}
