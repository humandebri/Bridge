// contracts/src: configure an OpenZeppelin timelock with a frozen canister operator role set.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

contract BridgeTimelockController is TimelockController {
    uint256 public constant MINIMUM_DELAY = 72 hours;

    error EmptyRoleMembers(bytes32 role);
    error ZeroRoleMember(bytes32 role);
    error MinimumDelayTooShort(uint256 suppliedDelay, uint256 minimumDelay);
    error RoleSetFrozen(bytes32 role, address account);

    bool private _rolePolicyActive;

    constructor(
        uint256 minimumDelay,
        address[] memory proposers,
        address[] memory cancellers,
        address[] memory executors
    ) TimelockController(minimumDelay, proposers, executors, address(0)) {
        if (minimumDelay < MINIMUM_DELAY) {
            revert MinimumDelayTooShort(minimumDelay, MINIMUM_DELAY);
        }
        _validateNonemptyRole(PROPOSER_ROLE, proposers);
        _validateNonemptyRole(CANCELLER_ROLE, cancellers);
        _validateNonemptyRole(EXECUTOR_ROLE, executors);

        // OpenZeppelin grants CANCELLER_ROLE to every proposer. Normalize the
        // role set to the explicit constructor list before freezing it. This
        // intentionally permits one canister-derived operator to hold all
        // three operational roles.
        for (uint256 index; index < proposers.length; ++index) {
            _revokeRole(CANCELLER_ROLE, proposers[index]);
        }
        for (uint256 index; index < cancellers.length; ++index) {
            _grantRole(CANCELLER_ROLE, cancellers[index]);
        }
        _rolePolicyActive = true;
    }

    function updateDelay(uint256 newDelay) public override {
        if (msg.sender != address(this)) {
            revert TimelockUnauthorizedCaller(msg.sender);
        }
        if (newDelay < MINIMUM_DELAY) {
            revert MinimumDelayTooShort(newDelay, MINIMUM_DELAY);
        }
        super.updateDelay(newDelay);
    }

    function _grantRole(bytes32 role, address account) internal override returns (bool changed) {
        if (_rolePolicyActive) {
            revert RoleSetFrozen(role, account);
        }

        changed = super._grantRole(role, account);
    }

    function _revokeRole(bytes32 role, address account) internal override returns (bool changed) {
        if (_rolePolicyActive) {
            revert RoleSetFrozen(role, account);
        }

        changed = super._revokeRole(role, account);
    }

    function _validateNonemptyRole(bytes32 role, address[] memory members) private pure {
        if (members.length == 0) {
            revert EmptyRoleMembers(role);
        }
        for (uint256 index; index < members.length; ++index) {
            if (members[index] == address(0)) {
                revert ZeroRoleMember(role);
            }
        }
    }
}
