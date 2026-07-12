// verification/smt/fail: prove the gate rejects a Runtime Administrator model that permits a limit increase.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract UnsafeAdministration {
    function increaseWindowLimit(uint256 currentWindowLimit, uint256 newWindowLimit) external pure {
        if (newWindowLimit > currentWindowLimit) {
            assert(newWindowLimit <= currentWindowLimit);
        }
    }
}
