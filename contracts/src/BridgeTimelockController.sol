// contracts/src: configure an OpenZeppelin timelock with an independently held cancellation role.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

contract BridgeTimelockController is TimelockController {
    uint256 public constant MINIMUM_DELAY = 72 hours;

    error EmptyRoleMembers(bytes32 role);
    error ZeroRoleMember(bytes32 role);
    error CancellerRoleOverlap(address account);
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

        // OpenZeppelin grants CANCELLER_ROLE to every proposer. Remove that
        // bootstrap convenience before assigning the independently held role.
        for (uint256 index; index < proposers.length; ++index) {
            _revokeRole(CANCELLER_ROLE, proposers[index]);
        }
        for (uint256 index; index < cancellers.length; ++index) {
            address canceller = cancellers[index];
            if (_contains(proposers, canceller) || _contains(executors, canceller)) {
                revert CancellerRoleOverlap(canceller);
            }
            _grantRole(CANCELLER_ROLE, canceller);
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

    function _contains(address[] memory members, address candidate) private pure returns (bool) {
        for (uint256 index; index < members.length; ++index) {
            if (members[index] == candidate) {
                return true;
            }
        }
        return false;
    }
}
