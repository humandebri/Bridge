// verification/smt/pass: prove the production Deposit net and fixed-window arithmetic on non-reverting paths.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {MintAccounting} from "bridge-src/libraries/MintAccounting.sol";

contract BoundedValue {
    function netAmount(uint256 grossAmount, uint256 serviceFee) external pure returns (uint256 result) {
        require(grossAmount > serviceFee);
        result = MintAccounting.netAmount(grossAmount, serviceFee);
        assert(result == grossAmount - serviceFee);
        assert(result > 0);
    }

    function consumeWindow(uint256 consumed, uint256 requested, uint256 limit) external pure returns (uint256 result) {
        require(consumed <= limit);
        (bool accepted, uint256 nextConsumed, uint256 available) =
            MintAccounting.tryConsumeWindow(consumed, requested, limit);
        if (accepted) {
            assert(nextConsumed >= consumed);
            assert(nextConsumed <= limit);
            assert(requested <= available);
        } else {
            assert(nextConsumed == consumed);
            assert(requested > available);
        }
        return nextConsumed;
    }
}
