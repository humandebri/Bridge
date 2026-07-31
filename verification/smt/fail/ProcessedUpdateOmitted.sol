// verification/smt/fail: a successful mint must consume the Deposit ID.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract ProcessedUpdateOmitted {
    function successfulMintLeavesDepositUnprocessed() external pure {
        bool accepted = true;
        bool processedAfter = false;
        assert(!accepted || processedAfter);
    }
}
