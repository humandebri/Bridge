// contracts/test: verify the OpenZeppelin TimelockController deployment and execution boundary.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {TimelockController} from "@openzeppelin/contracts/governance/TimelockController.sol";
import {IAccessControl} from "@openzeppelin/contracts/access/IAccessControl.sol";
import {Bridge} from "../src/Bridge.sol";
import {BridgeTimelockController} from "../src/BridgeTimelockController.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";
import {DeploymentPolicy} from "bridge-deployment-policy/DeploymentPolicy.sol";
import {
    ProductionTimelockPolicyNegativeFixture,
    StagingTimelockPolicyNegativeFixture
} from "./fixtures/TimelockPolicyNegativeFixtures.sol";

contract BridgeTimelockTest is TestBase {
    address private constant BRIDGE_SIGNER = address(0x11);
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant BASE_ADMIN_WALLET = address(0x33);
    address private constant OUTSIDER = address(0x44);
    address private constant CANCELLER = address(0x55);
    uint256 private constant TIMELOCK_DELAY = DeploymentPolicy.MINIMUM_TIMELOCK_DELAY;

    Bridge private bridge;
    BridgeTimelockController private timelock;
    uint256 private currentTimestamp;

    function setUp() public {
        currentTimestamp = block.timestamp;
        address[] memory proposers = new address[](1);
        proposers[0] = BASE_ADMIN_WALLET;
        address[] memory executors = new address[](1);
        executors[0] = BASE_ADMIN_WALLET;
        address[] memory cancellers = new address[](1);
        cancellers[0] = CANCELLER;
        timelock = new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);
        bridge = new Bridge(
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            address(timelock),
            _timelockCodeHash(address(timelock)),
            1_000,
            2_000,
            1 hours,
            1,
            100,
            10
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
        assert(timelock.roleMember(timelock.DEFAULT_ADMIN_ROLE()) == address(timelock));
        assert(timelock.roleMember(timelock.PROPOSER_ROLE()) == BASE_ADMIN_WALLET);
        assert(timelock.roleMember(timelock.CANCELLER_ROLE()) == CANCELLER);
        assert(timelock.roleMember(timelock.EXECUTOR_ROLE()) == BASE_ADMIN_WALLET);
        assert(timelock.pendingOperationCount() == 0);
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
        assert(timelock.pendingOperationCount() == 1);
        vm.expectPartialRevert(TimelockController.TimelockUnexpectedOperationState.selector);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);
        assert(timelock.pendingOperationCount() == 1);

        _advanceTime(TIMELOCK_DELAY);
        vm.expectRevert(
            abi.encodeWithSelector(IAccessControl.AccessControlUnauthorizedAccount.selector, OUTSIDER, executorRole)
        );
        vm.prank(OUTSIDER);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);

        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);
        assert(timelock.pendingOperationCount() == 0);
        assert(!bridge.depositMintsPaused());
    }

    function testOnlyIndependentCancellerCanCancel() public {
        bytes memory data = abi.encodeCall(IBridge.rotateRuntimeAdministrator, (address(0x55)));
        bytes32 salt = keccak256("cancel");
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(bridge), 0, data, bytes32(0), salt, TIMELOCK_DELAY);
        assert(timelock.pendingOperationCount() == 1);
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
        assert(timelock.pendingOperationCount() == 0);

        _advanceTime(TIMELOCK_DELAY);
        vm.expectPartialRevert(TimelockController.TimelockUnexpectedOperationState.selector);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(bridge), 0, data, bytes32(0), salt);
    }

    function testOperationalRoleRotationRequiresTheTimelockAndRotatesAllMembersAtomically() public {
        address nextGovernanceOperator = address(0x66);
        address nextCanceller = address(0x77);
        bytes memory data =
            abi.encodeCall(BridgeTimelockController.rotateOperationalMembers, (nextGovernanceOperator, nextCanceller));
        bytes32 salt = keccak256("control-plane-rotation");

        vm.expectRevert(
            abi.encodeWithSelector(TimelockController.TimelockUnauthorizedCaller.selector, BASE_ADMIN_WALLET)
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.rotateOperationalMembers(nextGovernanceOperator, nextCanceller);

        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(timelock), 0, data, bytes32(0), salt, TIMELOCK_DELAY);
        _advanceTime(TIMELOCK_DELAY);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(timelock), 0, data, bytes32(0), salt);

        assert(!timelock.hasRole(timelock.PROPOSER_ROLE(), BASE_ADMIN_WALLET));
        assert(!timelock.hasRole(timelock.EXECUTOR_ROLE(), BASE_ADMIN_WALLET));
        assert(!timelock.hasRole(timelock.CANCELLER_ROLE(), CANCELLER));
        assert(timelock.hasRole(timelock.PROPOSER_ROLE(), nextGovernanceOperator));
        assert(timelock.hasRole(timelock.EXECUTOR_ROLE(), nextGovernanceOperator));
        assert(timelock.hasRole(timelock.CANCELLER_ROLE(), nextCanceller));
        assert(timelock.roleMember(timelock.PROPOSER_ROLE()) == nextGovernanceOperator);
        assert(timelock.roleMember(timelock.EXECUTOR_ROLE()) == nextGovernanceOperator);
        assert(timelock.roleMember(timelock.CANCELLER_ROLE()) == nextCanceller);
    }

    function testControlPlaneRotationBatchHashMatchesCanisterAbiKnownAnswer() public view {
        address[] memory targets = new address[](3);
        targets[0] = address(0x0707070707070707070707070707070707070707);
        targets[1] = targets[0];
        targets[2] = address(0x0808080808080808080808080808080808080808);
        uint256[] memory values = new uint256[](3);
        bytes[] memory payloads = new bytes[](3);
        payloads[0] = abi.encodeCall(IBridge.rotateBridgeSigner, (address(0x0101010101010101010101010101010101010101)));
        payloads[1] =
            abi.encodeCall(IBridge.rotateRuntimeAdministrator, (address(0x0303030303030303030303030303030303030303)));
        payloads[2] = abi.encodeCall(
            BridgeTimelockController.rotateOperationalMembers,
            (address(0x0202020202020202020202020202020202020202), address(0x0404040404040404040404040404040404040404))
        );
        bytes32 operationId = timelock.hashOperationBatch(
            targets,
            values,
            payloads,
            bytes32(0),
            bytes32(uint256(0x0909090909090909090909090909090909090909090909090909090909090909))
        );
        assert(operationId == 0x0d3acc2286d377aaf6d0cd907239d72a480a50fa085600bdd815a202c0a7c1ba);
    }

    function testConstructorRequiresIndependentCancellerAndRejectsOpenRoles() public {
        address[] memory proposers = new address[](1);
        proposers[0] = BASE_ADMIN_WALLET;
        address[] memory cancellers = new address[](1);
        cancellers[0] = CANCELLER;
        address[] memory executors = new address[](1);
        executors[0] = BASE_ADMIN_WALLET;
        BridgeTimelockController separatedRoles =
            new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);
        assert(separatedRoles.hasRole(separatedRoles.PROPOSER_ROLE(), BASE_ADMIN_WALLET));
        assert(separatedRoles.hasRole(separatedRoles.CANCELLER_ROLE(), CANCELLER));
        assert(separatedRoles.hasRole(separatedRoles.EXECUTOR_ROLE(), BASE_ADMIN_WALLET));

        cancellers[0] = BASE_ADMIN_WALLET;
        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeTimelockController.CancellerMustBeIndependent.selector, BASE_ADMIN_WALLET, BASE_ADMIN_WALLET
            )
        );
        new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);
        cancellers[0] = CANCELLER;

        executors[0] = address(0);
        vm.expectRevert(
            abi.encodeWithSelector(BridgeTimelockController.ZeroRoleMember.selector, timelock.EXECUTOR_ROLE())
        );
        new BridgeTimelockController(TIMELOCK_DELAY, proposers, cancellers, executors);

        executors[0] = OUTSIDER;
        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeTimelockController.MinimumDelayTooShort.selector, TIMELOCK_DELAY - 1, TIMELOCK_DELAY
            )
        );
        new BridgeTimelockController(TIMELOCK_DELAY - 1, proposers, cancellers, executors);

        address[] memory duplicateProposers = new address[](2);
        duplicateProposers[0] = BASE_ADMIN_WALLET;
        duplicateProposers[1] = OUTSIDER;
        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeTimelockController.RoleMustHaveSingleMember.selector, timelock.PROPOSER_ROLE(), 2
            )
        );
        new BridgeTimelockController(TIMELOCK_DELAY, duplicateProposers, cancellers, executors);

        vm.expectRevert(
            abi.encodeWithSelector(BridgeTimelockController.MaximumDelayTooLong.selector, 30 days + 1, 30 days)
        );
        new BridgeTimelockController(30 days + 1, proposers, cancellers, executors);
    }

    function testSelectedBuildRejectsItsEnvironmentSpecificNegativeFixture() public {
        uint256 rejectedDelay;
        if (TIMELOCK_DELAY == ProductionTimelockPolicyNegativeFixture.MINIMUM_DELAY) {
            rejectedDelay = ProductionTimelockPolicyNegativeFixture.REJECTED_DELAY;
        } else {
            assert(TIMELOCK_DELAY == StagingTimelockPolicyNegativeFixture.MINIMUM_DELAY);
            rejectedDelay = StagingTimelockPolicyNegativeFixture.REJECTED_DELAY;
        }

        address[] memory operator = new address[](1);
        operator[0] = BASE_ADMIN_WALLET;
        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeTimelockController.MinimumDelayTooShort.selector, rejectedDelay, TIMELOCK_DELAY
            )
        );
        new BridgeTimelockController(rejectedDelay, operator, operator, operator);
    }

    function testDelayCannotBeReducedBelowPermanentMinimum() public {
        bytes32 defaultAdminRole = timelock.DEFAULT_ADMIN_ROLE();
        bytes32 executorRole = timelock.EXECUTOR_ROLE();
        vm.expectRevert(
            abi.encodeWithSelector(TimelockController.TimelockUnauthorizedCaller.selector, BASE_ADMIN_WALLET)
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.updateDelay(TIMELOCK_DELAY - 1);
        bytes memory delayData = abi.encodeCall(TimelockController.updateDelay, (TIMELOCK_DELAY - 1));
        bytes32 delaySalt = keccak256("delay");
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(timelock), 0, delayData, bytes32(0), delaySalt, TIMELOCK_DELAY);
        _advanceTime(TIMELOCK_DELAY);
        vm.expectRevert(
            abi.encodeWithSelector(
                BridgeTimelockController.MinimumDelayTooShort.selector, TIMELOCK_DELAY - 1, TIMELOCK_DELAY
            )
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(timelock), 0, delayData, bytes32(0), delaySalt);
        assert(timelock.getMinDelay() == TIMELOCK_DELAY);

        vm.expectRevert(
            abi.encodeWithSelector(
                IAccessControl.AccessControlUnauthorizedAccount.selector, BASE_ADMIN_WALLET, defaultAdminRole
            )
        );
        vm.prank(BASE_ADMIN_WALLET);
        timelock.grantRole(executorRole, OUTSIDER);
    }

    function testDelayCanIncreaseThroughNormalTimelockExecution() public {
        uint256 increasedDelay = TIMELOCK_DELAY + 1 days;
        bytes memory data = abi.encodeCall(TimelockController.updateDelay, (increasedDelay));
        bytes32 salt = keccak256("increase-delay");
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(timelock), 0, data, bytes32(0), salt, TIMELOCK_DELAY);
        _advanceTime(TIMELOCK_DELAY);
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(timelock), 0, data, bytes32(0), salt);
        assert(timelock.getMinDelay() == increasedDelay);
    }

    function testRoleSetIsFrozenAfterConstruction() public {
        bytes32 executorRole = timelock.EXECUTOR_ROLE();
        bytes memory grantExecutor = abi.encodeCall(IAccessControl.grantRole, (executorRole, OUTSIDER));
        _executeRoleMutationAndExpectFrozen(grantExecutor, keccak256("grant-executor"), executorRole, OUTSIDER);

        bytes32 proposerRole = timelock.PROPOSER_ROLE();
        bytes memory revokeProposer = abi.encodeCall(IAccessControl.revokeRole, (proposerRole, BASE_ADMIN_WALLET));
        _executeRoleMutationAndExpectFrozen(
            revokeProposer, keccak256("revoke-proposer"), proposerRole, BASE_ADMIN_WALLET
        );

        bytes32 defaultAdminRole = timelock.DEFAULT_ADMIN_ROLE();
        bytes memory revokeSelfAdmin = abi.encodeCall(IAccessControl.revokeRole, (defaultAdminRole, address(timelock)));
        _executeRoleMutationAndExpectFrozen(
            revokeSelfAdmin, keccak256("revoke-self-admin"), defaultAdminRole, address(timelock)
        );

        bytes32 cancellerRole = timelock.CANCELLER_ROLE();
        vm.expectRevert(
            abi.encodeWithSelector(BridgeTimelockController.RoleSetFrozen.selector, cancellerRole, CANCELLER)
        );
        vm.prank(CANCELLER);
        timelock.renounceRole(cancellerRole, CANCELLER);

        assert(!timelock.hasRole(executorRole, OUTSIDER));
        assert(timelock.hasRole(proposerRole, BASE_ADMIN_WALLET));
        assert(timelock.hasRole(defaultAdminRole, address(timelock)));
        assert(timelock.hasRole(cancellerRole, CANCELLER));
    }

    function _executeRoleMutationAndExpectFrozen(bytes memory data, bytes32 salt, bytes32 role, address account)
        private
    {
        vm.prank(BASE_ADMIN_WALLET);
        timelock.schedule(address(timelock), 0, data, bytes32(0), salt, TIMELOCK_DELAY);
        _advanceTime(TIMELOCK_DELAY);
        vm.expectRevert(abi.encodeWithSelector(BridgeTimelockController.RoleSetFrozen.selector, role, account));
        vm.prank(BASE_ADMIN_WALLET);
        timelock.execute(address(timelock), 0, data, bytes32(0), salt);
    }

    function _advanceTime(uint256 seconds_) private {
        // Track the mocked time explicitly because via-IR may treat
        // block.timestamp as invariant within a single test transaction.
        currentTimestamp += seconds_;
        vm.warp(currentTimestamp);
    }
}
