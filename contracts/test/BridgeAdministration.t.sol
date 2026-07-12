// contracts/test: verify Bridge role separation, safety controls, and rotations.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase, Vm} from "./TestBase.sol";

contract BridgeAdministrationTest is TestBase {
    address private constant BRIDGE_SIGNER = address(0x11);
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant BASE_ADMIN_TIMELOCK = address(0x33);
    address private constant USER = address(0x44);
    address private constant OUTSIDER = address(0x55);
    address private constant NEW_BRIDGE_SIGNER = address(0x66);
    address private constant NEW_RUNTIME_ADMINISTRATOR = address(0x77);
    address private constant NEW_TIMELOCK = address(0x88);
    address private constant SAFE = address(0x99);
    uint256 private constant MAX_SERVICE_FEE = 100;
    uint256 private constant SERVICE_FEE = 10;
    uint256 private constant PER_DEPOSIT_LIMIT = 1_000;
    uint256 private constant WINDOW_LIMIT = 2_000;
    uint64 private constant WINDOW_DURATION = 1 hours;

    event ServiceFeeChanged(address indexed caller, uint256 previousFee, uint256 newFee);
    event MintLimitsChanged(
        address indexed caller,
        uint256 previousPerDepositLimit,
        uint256 newPerDepositLimit,
        uint256 previousWindowLimit,
        uint256 newWindowLimit,
        uint64 previousWindowDuration,
        uint64 newWindowDuration
    );
    event DepositMintsPaused(address indexed caller);
    event DepositMintsUnpaused(address indexed caller);
    event WithdrawalsPaused(address indexed caller);
    event WithdrawalsUnpaused(address indexed caller);
    event BridgeSignerChanged(address indexed previousSigner, address indexed newSigner);
    event RuntimeAdministratorChanged(address indexed previousAdministrator, address indexed newAdministrator);
    event BaseAdminTimelockChanged(address indexed previousTimelock, address indexed newTimelock);

    Bridge private bridge;
    IBSNS private token;

    function setUp() public {
        bridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            PER_DEPOSIT_LIMIT,
            WINDOW_LIMIT,
            WINDOW_DURATION,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );
        token = bridge.bsns();
    }

    function testAdministrationFunctionsRejectEveryWrongAuthority() public {
        _assertRuntimeFunctionsRejected(BRIDGE_SIGNER);
        _assertRuntimeFunctionsRejected(BASE_ADMIN_TIMELOCK);
        _assertRuntimeFunctionsRejected(SAFE);
        _assertRuntimeFunctionsRejected(OUTSIDER);

        _assertTimelockFunctionsRejected(BRIDGE_SIGNER);
        _assertTimelockFunctionsRejected(RUNTIME_ADMINISTRATOR);
        _assertTimelockFunctionsRejected(SAFE);
        _assertTimelockFunctionsRejected(OUTSIDER);
    }

    function testPausesAreIndependentIdempotentAndBlockOnlyNewWork() public {
        _mintDeposit(keccak256("fund"), USER, 610, SERVICE_FEE, BRIDGE_SIGNER);
        vm.prank(USER);
        uint256 releaseId = bridge.createWithdrawal(300, 250, hex"01", bytes32(0));
        vm.prank(USER);
        uint256 refundId = bridge.createWithdrawal(200, 150, hex"02", bytes32(0));

        vm.expectEmit(true, false, false, true, address(bridge));
        emit DepositMintsPaused(RUNTIME_ADMINISTRATOR);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseDepositMints();
        assert(bridge.depositMintsPaused());
        assert(!bridge.withdrawalsPaused());

        vm.expectEmit(true, false, false, true, address(bridge));
        emit WithdrawalsPaused(RUNTIME_ADMINISTRATOR);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseWithdrawals();
        assert(bridge.withdrawalsPaused());

        vm.recordLogs();
        vm.startPrank(RUNTIME_ADMINISTRATOR);
        bridge.pauseDepositMints();
        bridge.pauseWithdrawals();
        vm.stopPrank();
        assert(vm.getRecordedLogs().length == 0);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(IBridge.DepositMintsArePaused.selector);
        bridge.mintDeposit(_request(keccak256("paused"), USER, 11, SERVICE_FEE));
        IBridge.DepositMintRequest[] memory batch = new IBridge.DepositMintRequest[](1);
        batch[0] = _request(keccak256("paused-batch"), USER, 11, SERVICE_FEE);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(IBridge.DepositMintsArePaused.selector);
        bridge.mintDeposits(batch);
        vm.prank(USER);
        vm.expectRevert(IBridge.WithdrawalsArePaused.selector);
        bridge.createWithdrawal(1, 1, hex"03", bytes32(0));

        vm.prank(BRIDGE_SIGNER);
        bridge.acknowledgeRelease(releaseId, 270, 10, 20, 1);
        vm.prank(BRIDGE_SIGNER);
        bridge.refundWithdrawal(refundId);
        assert(bridge.getWithdrawal(releaseId).status == IBridge.WithdrawalStatus.Released);
        assert(bridge.getWithdrawal(refundId).status == IBridge.WithdrawalStatus.Refunded);

        vm.expectEmit(true, false, false, true, address(bridge));
        emit DepositMintsUnpaused(BASE_ADMIN_TIMELOCK);
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseDepositMints();
        vm.expectEmit(true, false, false, true, address(bridge));
        emit WithdrawalsUnpaused(BASE_ADMIN_TIMELOCK);
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseWithdrawals();

        vm.recordLogs();
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseDepositMints();
        bridge.unpauseWithdrawals();
        vm.stopPrank();
        assert(vm.getRecordedLogs().length == 0);
    }

    function testServiceFeeAcceptsZeroAndMaximumAndAffectsFutureDeposits() public {
        vm.expectEmit(true, false, false, true, address(bridge));
        emit ServiceFeeChanged(RUNTIME_ADMINISTRATOR, SERVICE_FEE, 0);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(0);
        _mintDeposit(keccak256("zero-fee"), USER, 100, 0, BRIDGE_SIGNER);
        assert(token.balanceOf(USER) == 100);

        vm.expectEmit(true, false, false, true, address(bridge));
        emit ServiceFeeChanged(RUNTIME_ADMINISTRATOR, 0, MAX_SERVICE_FEE);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(MAX_SERVICE_FEE);
        _mintDeposit(keccak256("max-fee"), USER, 101, MAX_SERVICE_FEE, BRIDGE_SIGNER);
        assert(token.balanceOf(USER) == 101);

        vm.prank(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        bridge.setServiceFee(101);

        vm.recordLogs();
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(MAX_SERVICE_FEE);
        assert(vm.getRecordedLogs().length == 0);
    }

    function testCurrentServiceFeeDoesNotRewritePendingWithdrawalSettlement() public {
        _mintDeposit(keccak256("withdrawal-fee"), USER, 210, SERVICE_FEE, BRIDGE_SIGNER);
        vm.prank(USER);
        uint256 withdrawalId = bridge.createWithdrawal(200, 150, hex"01", bytes32(0));
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(MAX_SERVICE_FEE);
        vm.prank(BRIDGE_SIGNER);
        bridge.acknowledgeRelease(withdrawalId, 170, 20, 10, 1);

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(bridge.serviceFee() == MAX_SERVICE_FEE);
        assert(withdrawal.serviceFee == 20);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Released);
    }

    function testRuntimeAdministratorCanOnlyMoveLimitsInSafeDirection() public {
        uint64 startedAt = bridge.mintWindowStartedAt();
        vm.expectEmit(true, false, false, true, address(bridge));
        emit MintLimitsChanged(
            RUNTIME_ADMINISTRATOR, PER_DEPOSIT_LIMIT, 900, WINDOW_LIMIT, 1_800, WINDOW_DURATION, WINDOW_DURATION + 1
        );
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.reduceMintLimits(900, 1_800, WINDOW_DURATION + 1);
        assert(bridge.perDepositLimit() == 900);
        assert(bridge.mintWindowLimit() == 1_800);
        assert(bridge.mintWindowDuration() == WINDOW_DURATION + 1);
        assert(bridge.mintWindowStartedAt() == startedAt);
        assert(bridge.mintedInWindow() == 0);

        _expectUnsafeLimitChange(901, 1_800, WINDOW_DURATION + 1);
        _expectUnsafeLimitChange(900, 1_801, WINDOW_DURATION + 1);
        _expectUnsafeLimitChange(900, 1_800, WINDOW_DURATION);
        _expectUnsafeLimitChange(900, 1_800, WINDOW_DURATION + 1);

        vm.prank(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        bridge.reduceMintLimits(0, 1_800, WINDOW_DURATION + 1);
    }

    function testBaseAdminCanSetArbitraryNonzeroLimitsWithoutResettingWindow() public {
        _mintDeposit(keccak256("consumed"), USER, 110, SERVICE_FEE, BRIDGE_SIGNER);
        uint64 startedAt = bridge.mintWindowStartedAt();
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.setMintLimits(50, 50, 1);
        assert(bridge.mintedInWindow() == 100);
        assert(bridge.mintWindowStartedAt() == startedAt);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.DepositMintLimitExceeded.selector, 51, 50));
        bridge.mintDeposit(_request(keccak256("per-limit"), USER, 61, SERVICE_FEE));
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintWindowLimitExceeded.selector, 1, 0));
        bridge.mintDeposit(_request(keccak256("window-limit"), USER, 11, SERVICE_FEE));

        vm.warp(uint256(startedAt) + 1);
        _mintDeposit(keccak256("rolled"), USER, 11, SERVICE_FEE, BRIDGE_SIGNER);
        assert(bridge.mintedInWindow() == 1);
        assert(bridge.mintWindowStartedAt() == startedAt + 1);

        vm.recordLogs();
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.setMintLimits(50, 50, 1);
        assert(vm.getRecordedLogs().length == 0);

        vm.prank(BASE_ADMIN_TIMELOCK);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        bridge.setMintLimits(50, 0, 1);
    }

    function testRoleRotationsRejectInvalidSetsAndImmediatelyRevokeOldAuthority() public {
        vm.expectEmit(true, true, false, true, address(bridge));
        emit BridgeSignerChanged(BRIDGE_SIGNER, NEW_BRIDGE_SIGNER);
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.rotateBridgeSigner(NEW_BRIDGE_SIGNER);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, BRIDGE_SIGNER));
        bridge.mintDeposit(_request(keccak256("old-signer"), USER, 11, SERVICE_FEE));
        _mintDeposit(keccak256("new-signer"), USER, 11, SERVICE_FEE, NEW_BRIDGE_SIGNER);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit RuntimeAdministratorChanged(RUNTIME_ADMINISTRATOR, NEW_RUNTIME_ADMINISTRATOR);
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.rotateRuntimeAdministrator(NEW_RUNTIME_ADMINISTRATOR);
        vm.prank(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(
            abi.encodeWithSelector(IBridge.UnauthorizedRuntimeAdministrator.selector, RUNTIME_ADMINISTRATOR)
        );
        bridge.pauseDepositMints();
        vm.prank(NEW_RUNTIME_ADMINISTRATOR);
        bridge.pauseDepositMints();

        vm.expectEmit(true, true, false, true, address(bridge));
        emit BaseAdminTimelockChanged(BASE_ADMIN_TIMELOCK, NEW_TIMELOCK);
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.rotateBaseAdminTimelock(NEW_TIMELOCK);
        vm.prank(BASE_ADMIN_TIMELOCK);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, BASE_ADMIN_TIMELOCK));
        bridge.unpauseDepositMints();
        vm.prank(NEW_TIMELOCK);
        bridge.unpauseDepositMints();

        vm.prank(NEW_TIMELOCK);
        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.rotateBridgeSigner(address(0));
        vm.prank(NEW_TIMELOCK);
        vm.expectRevert(IBridge.RoleAddressesMustDiffer.selector);
        bridge.rotateBridgeSigner(NEW_RUNTIME_ADMINISTRATOR);
    }

    function testEveryRotationRejectsZeroAndRoleCollisions() public {
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.rotateBridgeSigner(address(0));
        vm.expectRevert(IBridge.RoleAddressesMustDiffer.selector);
        bridge.rotateBridgeSigner(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.rotateRuntimeAdministrator(address(0));
        vm.expectRevert(IBridge.RoleAddressesMustDiffer.selector);
        bridge.rotateRuntimeAdministrator(BRIDGE_SIGNER);
        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.rotateBaseAdminTimelock(address(0));
        vm.expectRevert(IBridge.RoleAddressesMustDiffer.selector);
        bridge.rotateBaseAdminTimelock(BRIDGE_SIGNER);
        vm.stopPrank();
    }

    function testAdministrativeAuthoritiesHaveNoSignerAssetAuthority() public {
        _assertSignerFunctionsRejected(RUNTIME_ADMINISTRATOR);
        _assertSignerFunctionsRejected(BASE_ADMIN_TIMELOCK);
    }

    function testSameRoleRotationsAreIdempotentWithoutEvents() public {
        vm.recordLogs();
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        bridge.rotateBridgeSigner(BRIDGE_SIGNER);
        bridge.rotateRuntimeAdministrator(RUNTIME_ADMINISTRATOR);
        bridge.rotateBaseAdminTimelock(BASE_ADMIN_TIMELOCK);
        vm.stopPrank();
        assert(vm.getRecordedLogs().length == 0);
    }

    function _assertRuntimeFunctionsRejected(address caller) private {
        vm.startPrank(caller);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedRuntimeAdministrator.selector, caller));
        bridge.pauseDepositMints();
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedRuntimeAdministrator.selector, caller));
        bridge.pauseWithdrawals();
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedRuntimeAdministrator.selector, caller));
        bridge.reduceMintLimits(900, 1_900, WINDOW_DURATION + 1);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedRuntimeAdministrator.selector, caller));
        bridge.setServiceFee(1);
        vm.stopPrank();
    }

    function _assertTimelockFunctionsRejected(address caller) private {
        vm.startPrank(caller);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.unpauseDepositMints();
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.unpauseWithdrawals();
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.setMintLimits(1, 1, 1);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.rotateBridgeSigner(NEW_BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.rotateRuntimeAdministrator(NEW_RUNTIME_ADMINISTRATOR);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.rotateBaseAdminTimelock(NEW_TIMELOCK);
        vm.stopPrank();
    }

    function _assertSignerFunctionsRejected(address caller) private {
        vm.startPrank(caller);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, caller));
        bridge.mintDeposit(_request(keccak256(abi.encode(caller, "mint")), USER, 11, SERVICE_FEE));
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, caller));
        bridge.acknowledgeRelease(1, 1, 0, 0, 1);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, caller));
        bridge.refundWithdrawal(1);
        vm.stopPrank();
    }

    function _expectUnsafeLimitChange(uint256 perDeposit, uint256 window, uint64 duration) private {
        vm.prank(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(IBridge.UnsafeLimitChange.selector);
        bridge.reduceMintLimits(perDeposit, window, duration);
    }

    function _mintDeposit(bytes32 depositId, address recipient, uint256 grossAmount, uint256 maximumFee, address signer)
        private
    {
        vm.prank(signer);
        bridge.mintDeposit(_request(depositId, recipient, grossAmount, maximumFee));
    }

    function _request(bytes32 depositId, address recipient, uint256 grossAmount, uint256 maximumFee)
        private
        pure
        returns (IBridge.DepositMintRequest memory)
    {
        return IBridge.DepositMintRequest(depositId, recipient, grossAmount, maximumFee);
    }
}
