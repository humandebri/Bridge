// contracts/src/libraries: share checked Deposit mint arithmetic with Bridge and SMT fixtures.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

library MintAccounting {
    function netAmount(uint256 grossAmount, uint256 serviceFee) internal pure returns (uint256) {
        return grossAmount - serviceFee;
    }

    function tryConsumeWindow(uint256 consumed, uint256 requested, uint256 limit)
        internal
        pure
        returns (bool accepted, uint256 nextConsumed, uint256 available)
    {
        available = consumed >= limit ? 0 : limit - consumed;
        if (requested > available) {
            return (false, consumed, available);
        }
        return (true, consumed + requested, available);
    }
}
