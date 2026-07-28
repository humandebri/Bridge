// contracts/src: configure an OpenZeppelin timelock with a frozen canister operator role set.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";

contract BridgeTimelockController is TimelockController {
    uint256 public constant MINIMUM_DELAY = 24 hours;
    uint256 public constant MAXIMUM_DELAY = 30 days;

    error EmptyRoleMembers(bytes32 role);
    error ZeroRoleMember(bytes32 role);
    error MinimumDelayTooShort(uint256 suppliedDelay, uint256 minimumDelay);
    error MaximumDelayTooLong(uint256 suppliedDelay, uint256 maximumDelay);
    error RoleMustHaveSingleMember(bytes32 role, uint256 suppliedMemberCount);
    error RoleSetFrozen(bytes32 role, address account);

    bool private _rolePolicyActive;
    mapping(bytes32 role => address member) private _roleMember;
    uint256 public pendingOperationCount;

    constructor(
        uint256 minimumDelay,
        address[] memory proposers,
        address[] memory cancellers,
        address[] memory executors
    ) TimelockController(minimumDelay, proposers, executors, address(0)) {
        if (minimumDelay < MINIMUM_DELAY) {
            revert MinimumDelayTooShort(minimumDelay, MINIMUM_DELAY);
        }
        if (minimumDelay > MAXIMUM_DELAY) {
            revert MaximumDelayTooLong(minimumDelay, MAXIMUM_DELAY);
        }
        _validateSingleRoleMember(PROPOSER_ROLE, proposers);
        _validateSingleRoleMember(CANCELLER_ROLE, cancellers);
        _validateSingleRoleMember(EXECUTOR_ROLE, executors);

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
        _roleMember[DEFAULT_ADMIN_ROLE] = address(this);
        _roleMember[PROPOSER_ROLE] = proposers[0];
        _roleMember[CANCELLER_ROLE] = cancellers[0];
        _roleMember[EXECUTOR_ROLE] = executors[0];
        _rolePolicyActive = true;
    }

    function roleMember(bytes32 role) external view returns (address) {
        return _roleMember[role];
    }

    function schedule(
        address target,
        uint256 value,
        bytes calldata data,
        bytes32 predecessor,
        bytes32 salt,
        uint256 delay
    ) public override {
        super.schedule(target, value, data, predecessor, salt, delay);
        ++pendingOperationCount;
    }

    function scheduleBatch(
        address[] calldata targets,
        uint256[] calldata values,
        bytes[] calldata payloads,
        bytes32 predecessor,
        bytes32 salt,
        uint256 delay
    ) public override {
        super.scheduleBatch(targets, values, payloads, predecessor, salt, delay);
        ++pendingOperationCount;
    }

    function cancel(bytes32 id) public override {
        super.cancel(id);
        --pendingOperationCount;
    }

    function execute(address target, uint256 value, bytes calldata payload, bytes32 predecessor, bytes32 salt)
        public
        payable
        override
    {
        super.execute(target, value, payload, predecessor, salt);
        --pendingOperationCount;
    }

    function executeBatch(
        address[] calldata targets,
        uint256[] calldata values,
        bytes[] calldata payloads,
        bytes32 predecessor,
        bytes32 salt
    ) public payable override {
        super.executeBatch(targets, values, payloads, predecessor, salt);
        --pendingOperationCount;
    }

    function updateDelay(uint256 newDelay) public override {
        if (msg.sender != address(this)) {
            revert TimelockUnauthorizedCaller(msg.sender);
        }
        if (newDelay < MINIMUM_DELAY) {
            revert MinimumDelayTooShort(newDelay, MINIMUM_DELAY);
        }
        if (newDelay > MAXIMUM_DELAY) {
            revert MaximumDelayTooLong(newDelay, MAXIMUM_DELAY);
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

    function _validateSingleRoleMember(bytes32 role, address[] memory members) private pure {
        if (members.length != 1) {
            revert RoleMustHaveSingleMember(role, members.length);
        }
        for (uint256 index; index < members.length; ++index) {
            if (members[index] == address(0)) {
                revert ZeroRoleMember(role);
            }
        }
    }
}
