// verification/smt/fail: mint, supply, and event amounts must be identical.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract SupplyEventMismatch {
    function successfulMintReportsDifferentAmount(uint256 mintAmount) external pure {
        require(mintAmount < type(uint256).max);
        uint256 supplyIncrease = mintAmount;
        uint256 eventAmount = mintAmount + 1;
        assert(supplyIncrease == eventAmount);
    }
}
