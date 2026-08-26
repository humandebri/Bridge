// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

interface ITimelockScheduler {
    function schedule(
        address target,
        uint256 value,
        bytes calldata data,
        bytes32 predecessor,
        bytes32 salt,
        uint256 delay
    ) external;
}

/// @dev Verification-only target that models a role-holding contract reentering schedule during execute.
contract TimelockNestedOperationTarget {
    function scheduleNested(
        address timelock,
        address target,
        uint256 value,
        bytes calldata data,
        bytes32 predecessor,
        bytes32 salt,
        uint256 delay
    ) external {
        ITimelockScheduler(timelock).schedule(target, value, data, predecessor, salt, delay);
    }
}
