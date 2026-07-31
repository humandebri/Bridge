// verification/smt/fail: values outside the stable u128 envelope must not be accepted.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract U128BoundOmitted {
    function acceptsValueAboveU128() external pure {
        uint256 grossAmount = uint256(type(uint128).max) + 1;
        bool accepted = true;
        assert(!accepted || grossAmount <= type(uint128).max);
    }
}
