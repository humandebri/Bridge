// verification/smt/pass: ensure CHC can prove a value accepted under an explicit upper bound.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract BoundedValue {
    uint256 public immutable MAXIMUM;

    constructor(uint256 initialMaximum) {
        MAXIMUM = initialMaximum;
    }

    function accept(uint256 value) external view returns (uint256) {
        require(value <= MAXIMUM);
        assert(value <= MAXIMUM);
        return value;
    }
}
