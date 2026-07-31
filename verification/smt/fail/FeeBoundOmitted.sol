// verification/smt/fail: a charged fee above either bound must not be accepted.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract FeeBoundOmitted {
    function acceptsFeeAboveMaximum(uint256 maximumFee) external pure {
        require(maximumFee < type(uint256).max);
        uint256 chargedFee = maximumFee + 1;
        bool accepted = true;
        assert(!accepted || chargedFee <= maximumFee);
    }
}
