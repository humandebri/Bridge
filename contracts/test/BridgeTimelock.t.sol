// contracts/test: verify the OpenZeppelin TimelockController deployment and execution boundary.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {IAccessControl} from "@openzeppelin/contracts/access/IAccessControl.sol";
import {Bridge} from "../src/Bridge.sol";
import {BridgeTimelockController} from "../src/BridgeTimelockController.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";

contract BridgeTimelockTest is TestBase {
    address private constant BRIDGE_SIGNER = address(0x11);
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant BASE_ADMIN_WALLET = address(0x33);
    address private constant OUTSIDER = address(0x44);
    address private constant CANCELLER = address(0x55);
    uint256 private constant TIMELOCK_DELAY = 72 hours;

    Bridge private bridge;
    BridgeTimelockController private timelock;

    function setUp() public {
        address[] memory proposers = new address[](1);
        proposers[0] = BASE_ADMIN_WALLET;
        address[] memory executors = new address[](1);
        executors[0] = BASE_ADMIN_WALLET;
        address[] memory cancellers = new address[](1);
        cancellers[0] = CANCELLER;
        timelock = new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);
        bridge = new Bridge(
            "kinic", "KINIC", 8, BRIDGE_SIGNER, RUNTIME_ADMINISTRATOR, address(timelock), 1_000, 2_000, 1 hours, 100, 10
        );
    }

    function testInitialConfigurationHasSingleBaseAdminWalletAndNoExternalAdmin() public view {
        assert(timelock.getMinDelay() == TIMELOCK_DELAY);
        assert(timelock.hasRole(timelock.PROPOSER_ROLE(), BASE_ADMIN_WALLET));
        assert(!timelock.hasRole(timelock.CANCELLER_ROLE(), BASE_ADMIN_WALLET));
        assert(timelock.hasRole(timelock.CANCELLER_ROLE(), CANCELLER));
        assert(timelock.hasRole(timelock.EXECUTOR_ROLE(), BASE_ADMIN_WALLET));
        assert(!timelock.hasRole(timelock.PROPOSER_ROLE(), OUTSIDER));
        assert(!timelock.hasRole(timelock.EXECUTOR_ROLE(), address(0)));
        assert(timelock.hasRole(timelock.DEFAULT_ADMIN_ROLE(), address(timelock)));
        assert(!timelock.hasRole(timelock.DEFAULT_ADMIN_ROLE(), BASE_ADMIN_WALLET));
        assert(bridge.baseAdminTimelock() == address(timelock));
        assert(bridge.depositMintsPaused());
        assert(bridge.withdrawalsPaused());
    }

    function testRequiresBaseAdminWalletAndFullDelayBeforeExecutingBridgeCall() public {
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseDepositMints();
        bytes memory data = abi.encodeCall(IBridge.unpauseDepositMints, ());
        bytes32 salt = keccak256("unpause");
        bytes32 proposerRole = timelock.PROPOSER_ROLE();
        bytes32 executorRole = timelock.EXECUTOR_ROLE();

        vm.expectRevert(
            abi.encodeWithSelector(IAccessControl.AccessControlUnauthorizedAccount.selector, OUTSIDER, proposerRole)
        );
        vm.prank(OUTSIDER);
        timelock.schedule(address(bridge), 0, data, bytes32(0), salt, TIMELOCK_DELAY);

        vm.expectRevert(
            abi.encodeWithSelector(
                TimelockController.TimelockInsufficientDelay.selector, TIMELOCK_DELAY - 1, TIMELOCK_DELAY
            )
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(bridge), 0, data, bytes32(0), salt, TIMELOCK_DELAY - 1);

        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(bridge), 0, data, bytes32(0), salt, TIMELOCK_DELAY);
        vm.expectPartialRevert(TimelockController.TimelockUnexpectedOperationState.selector);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);

        vm.warp(block.timestamp + TIMELOCK_DELAY);
        vm.expectRevert(
            abi.encodeWithSelector(IAccessControl.AccessControlUnauthorizedAccount.selector, OUTSIDER, executorRole)
        );
        vm.prank(OUTSIDER);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);

        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);
        assert(!bridge.depositMintsPaused());
    }

    function testOnlyIndependentCancellerCanCancel() public {
        bytes memory data = abi.encodeCall(IBridge.rotateRuntimeAdministrator, (address(0x55)));
        bytes32 salt = keccak256("cancel");
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(bridge), 0, data, bytes32(0), salt, TIMELOCK_DELAY);
        bytes32 operationId = timelock.hashOperation(address(bridge), 0, data, bytes32(0), salt);
        bytes32 cancellerRole = timelock.CANCELLER_ROLE();

        vm.expectRevert(
            abi.encodeWithSelector(IAccessControl.AccessControlUnauthorizedAccount.selector, OUTSIDER, cancellerRole)
        );
        vm.prank(OUTSIDER);
        timelock.cancel(operationId);
        vm.expectRevert(
            abi.encodeWithSelector(
                IAccessControl.AccessControlUnauthorizedAccount.selector, BASE_ADMIN_WALLET, cancellerRole
            )
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.cancel(operationId);
        vm.prank(CANCELLER);
        timelock.cancel(operationId);

        vm.warp(block.timestamp + TIMELOCK_DELAY);
        vm.expectPartialRevert(TimelockController.TimelockUnexpectedOperationState.selector);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);
    }

    function testConstructorRejectsCancellerOverlapAndOpenRoles() public {
        address[] memory proposers = new address[](1);
        proposers[0] = BASE_ADMIN_WALLET;
        address[] memory cancellers = new address[](1);
        cancellers[0] = BASE_ADMIN_WALLET;
        address[] memory executors = new address[](1);
        executors[0] = OUTSIDER;
        vm.expectRevert(
            abi.encodeWithSelector(BridgeTimelockController.CancellerRoleOverlap.selector, BASE_ADMIN_WALLET)
        );
        new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);

        cancellers[0] = CANCELLER;
        executors[0] = CANCELLER;
        vm.expectRevert(abi.encodeWithSelector(BridgeTimelockController.CancellerRoleOverlap.selector, CANCELLER));
        new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);

        executors[0] = address(0);
        vm.expectRevert(
            abi.encodeWithSelector(BridgeTimelockController.ZeroRoleMember.selector, timelock.EXECUTOR_ROLE())
        );
        new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);
    }

    function testDelayAndRoleChangesRequireTimelockSelfCall() public {
        bytes32 defaultAdminRole = timelock.DEFAULT_ADMIN_ROLE();
        bytes32 executorRole = timelock.EXECUTOR_ROLE();
        vm.expectRevert(
            abi.encodeWithSelector(TimelockController.TimelockUnauthorizedCaller.selector, BASE_ADMIN_WALLET)
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.updateDelay(24 hours);
        vm.expectRevert(
            abi.encodeWithSelector(
                IAccessControl.AccessControlUnauthorizedAccount.selector, BASE_ADMIN_WALLET, defaultAdminRole
            )
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.grantRole(executorRole, OUTSIDER);

        bytes memory delayData = abi.encodeCall(TimelockController.updateDelay, (24 hours));
        bytes32 delaySalt = keccak256("delay");
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(timelock), 0, delayData, bytes32(0), delaySalt, TIMELOCK_DELAY);
        vm.warp(block.timestamp + TIMELOCK_DELAY);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(timelock), 0, delayData, bytes32(0), delaySalt);
        assert(timelock.getMinDelay() == 24 hours);

        bytes memory roleData = abi.encodeCall(IAccessControl.grantRole, (executorRole, OUTSIDER));
        bytes32 roleSalt = keccak256("role");
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(timelock), 0, roleData, bytes32(0), roleSalt, 24 hours);
        vm.warp(block.timestamp + 24 hours);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(timelock), 0, roleData, bytes32(0), roleSalt);
        assert(timelock.hasRole(executorRole, OUTSIDER));
    }
}
