// contracts/test: verify Bridge role separation, safety controls, and rotations.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase, Vm} from "./TestBase.sol";

contract TimelockCandidateFixture {
    // Storage-backed values keep the runtime code hash identical across candidates so
    // Bridge tests can reach the post-code-hash delay and self-admin checks.
    uint256 private _delay;
    bool private _selfAdmin;
    address private constant OPERATOR = address(0xBEEF);

    constructor(uint256 delay, bool selfAdmin) {
        _delay = delay;
        _selfAdmin = selfAdmin;
    }

    function getMinDelay() external view returns (uint256) {
        return _delay;
    }

    function hasRole(bytes32 role, address account) external view returns (bool) {
        if (role == bytes32(0)) {
            return _selfAdmin && account == address(this);
        }
        return account == OPERATOR;
    }

    function roleMember(bytes32 role) external view returns (address) {
        return role == bytes32(0) ? address(this) : OPERATOR;
    }

    function pendingOperationCount() external pure returns (uint256) {
        return 0;
    }
}

contract BridgeAdministrationTest is TestBase {
    uint256 private constant BRIDGE_SIGNER_KEY = 0xA11CE;
    uint256 private constant NEW_BRIDGE_SIGNER_KEY = 0xB0B;
    address private BRIDGE_SIGNER;
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private BASE_ADMIN_TIMELOCK;
    address private constant USER = address(0x44);
    address private constant OUTSIDER = address(0x55);
    address private NEW_BRIDGE_SIGNER;
    address private constant NEW_RUNTIME_ADMINISTRATOR = address(0x77);
    address private NEW_TIMELOCK;
    address private constant BASE_ADMIN_WALLET = address(0x99);
    uint256 private constant MAX_SERVICE_FEE = 100;
    uint256 private constant SERVICE_FEE = 10;
    uint256 private constant PER_DEPOSIT_LIMIT = 1_000;
    uint256 private constant WINDOW_LIMIT = 2_000;
    uint64 private constant WINDOW_DURATION = 1 hours;

    event ServiceFeeChanged(address indexed caller, uint256 previousFee, uint256 newFee);
    event DepositMintsPaused(address indexed caller);
    event DepositMintsUnpaused(address indexed caller);
    event WithdrawalsPaused(address indexed caller);
    event WithdrawalsUnpaused(address indexed caller);
    event BridgeSignerChanged(address indexed previousSigner, address indexed newSigner);
    event MintAuthorizationEpochChanged(address indexed caller, uint256 previousEpoch, uint256 newEpoch);
    event RuntimeAdministratorChanged(address indexed previousAdministrator, address indexed newAdministrator);
    event BaseAdminTimelockChanged(address indexed previousTimelock, address indexed newTimelock);

    Bridge private bridge;
    IBSNS private token;

    function setUp() public {
        BRIDGE_SIGNER = vm.addr(BRIDGE_SIGNER_KEY);
        NEW_BRIDGE_SIGNER = vm.addr(NEW_BRIDGE_SIGNER_KEY);
        BASE_ADMIN_TIMELOCK = _deployTestTimelock(address(0x33));
        NEW_TIMELOCK = _deployTestTimelock(address(0x34));
        bridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            PER_DEPOSIT_LIMIT,
            WINDOW_LIMIT,
            WINDOW_DURATION,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );
        token = bridge.bsns();
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseDepositMints();
        bridge.unpauseWithdrawals();
        vm.stopPrank();
    }

    function testConstructorStartsBothAssetFlowsPaused() public {
        Bridge freshBridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            PER_DEPOSIT_LIMIT,
            WINDOW_LIMIT,
            WINDOW_DURATION,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );
        assert(freshBridge.depositMintsPaused());
        assert(freshBridge.withdrawalsPaused());
    }

    function testMintRejectsBlockTimestampThatCannotBeStoredInWindowState() public {
        IBridge.MintAuthorization memory authorization =
            _authorization(keccak256("timestamp-overflow"), USER, 11, SERVICE_FEE);
        authorization.deadline = type(uint256).max;
        vm.warp(uint256(type(uint64).max) + 1);
        bytes memory signature = _signMintAuthorization(BRIDGE_SIGNER_KEY, bridge, authorization);

        vm.expectRevert(
            abi.encodeWithSelector(IBridge.BlockTimestampExceedsU64.selector, block.timestamp)
        );
        bridge.mintDepositWithAuthorization(authorization, signature);
    }

    function testAdministrationFunctionsRejectEveryWrongAuthority() public {
        _assertRuntimeFunctionsRejected(BRIDGE_SIGNER);
        _assertRuntimeFunctionsRejected(BASE_ADMIN_TIMELOCK);
        _assertRuntimeFunctionsRejected(BASE_ADMIN_WALLET);
        _assertRuntimeFunctionsRejected(OUTSIDER);

        _assertTimelockFunctionsRejected(BRIDGE_SIGNER);
        _assertTimelockFunctionsRejected(RUNTIME_ADMINISTRATOR);
        _assertTimelockFunctionsRejected(BASE_ADMIN_WALLET);
        _assertTimelockFunctionsRejected(OUTSIDER);
    }

    function testPausesAreIndependentIdempotentAndBlockOnlyNewWork() public {
        _mintAuthorized(keccak256("fund"), USER, 610, SERVICE_FEE, BRIDGE_SIGNER_KEY);
        vm.prank(USER);
        token.approve(address(bridge), 300);
        vm.prank(USER);
        uint256 releaseId = bridge.createWithdrawal(300, SERVICE_FEE, hex"01", bytes32(0));
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

        IBridge.MintAuthorization memory pausedAuthorization =
            _authorization(keccak256("paused"), USER, 11, SERVICE_FEE);
        bytes memory pausedSignature = _signMintAuthorization(BRIDGE_SIGNER_KEY, bridge, pausedAuthorization);
        vm.expectRevert(IBridge.DepositMintsArePaused.selector);
        bridge.mintDepositWithAuthorization(pausedAuthorization, pausedSignature);
        vm.prank(USER);
        vm.expectRevert(IBridge.WithdrawalsArePaused.selector);
        bridge.createWithdrawal(1, 1, hex"03", bytes32(0));

        assert(bridge.getWithdrawal(releaseId).status == IBridge.WithdrawalStatus.Committed);

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
        _mintDepositWithChargedFee(keccak256("zero-fee"), USER, 100, 0, 0, BRIDGE_SIGNER_KEY);
        assert(token.balanceOf(USER) == 100);

        vm.expectEmit(true, false, false, true, address(bridge));
        emit ServiceFeeChanged(RUNTIME_ADMINISTRATOR, 0, MAX_SERVICE_FEE);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(MAX_SERVICE_FEE);
        _mintDepositWithChargedFee(keccak256("max-fee"), USER, 101, MAX_SERVICE_FEE, MAX_SERVICE_FEE, BRIDGE_SIGNER_KEY);
        assert(token.balanceOf(USER) == 101);

        vm.prank(RUNTIME_ADMINISTRATOR);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        bridge.setServiceFee(101);

        vm.recordLogs();
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(MAX_SERVICE_FEE);
        assert(vm.getRecordedLogs().length == 0);
    }

    function testCurrentServiceFeeDoesNotRewriteCommittedWithdrawalQuote() public {
        _mintAuthorized(keccak256("withdrawal-fee"), USER, 210, SERVICE_FEE, BRIDGE_SIGNER_KEY);
        vm.prank(USER);
        token.approve(address(bridge), 200);
        vm.prank(USER);
        uint256 withdrawalId = bridge.createWithdrawal(200, SERVICE_FEE, hex"01", bytes32(0));
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(MAX_SERVICE_FEE);
        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(bridge.serviceFee() == MAX_SERVICE_FEE);
        assert(withdrawal.chargedServiceFee == SERVICE_FEE);
        assert(withdrawal.amountOut == 190);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Committed);
    }

    function testAdministrationDoesNotChangeConstructorMintLimits() public {
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(20);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseDepositMints();
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseDepositMints();

        assert(bridge.perDepositLimit() == PER_DEPOSIT_LIMIT);
        assert(bridge.mintWindowLimit() == WINDOW_LIMIT);
        assert(bridge.mintWindowDuration() == WINDOW_DURATION);
    }

    function testRoleRotationsRejectInvalidSetsAndImmediatelyRevokeOldAuthority() public {
        IBridge.MintAuthorization memory oldAuthorization =
            _authorization(keccak256("old-signer"), USER, 11, SERVICE_FEE);
        vm.expectEmit(true, true, false, true, address(bridge));
        emit BridgeSignerChanged(BRIDGE_SIGNER, NEW_BRIDGE_SIGNER);
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.rotateBridgeSigner(NEW_BRIDGE_SIGNER);
        assert(bridge.mintAuthorizationEpoch() == 2);
        vm.expectRevert(IBridge.InvalidMintAuthorizationSignature.selector);
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, bridge, oldAuthorization, address(this));
        _mintAuthorized(keccak256("new-signer"), USER, 11, SERVICE_FEE, NEW_BRIDGE_SIGNER_KEY);

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

    function testConstructorRejectsUnapprovedInitialTimelock() public {
        vm.expectRevert(abi.encodeWithSelector(IBridge.TimelockCandidateHasNoCode.selector, OUTSIDER));
        new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            OUTSIDER,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            PER_DEPOSIT_LIMIT,
            WINDOW_LIMIT,
            WINDOW_DURATION,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );

        address spoof = address(new TimelockCandidateFixture(24 hours, true));
        vm.expectPartialRevert(IBridge.TimelockCandidateCodeHashMismatch.selector);
        new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            spoof,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            PER_DEPOSIT_LIMIT,
            WINDOW_LIMIT,
            WINDOW_DURATION,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );
    }

    function testTimelockRotationRejectsEoaAndInterfaceSpoofs() public {
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        vm.expectRevert(abi.encodeWithSelector(IBridge.TimelockCandidateHasNoCode.selector, OUTSIDER));
        bridge.rotateBaseAdminTimelock(OUTSIDER);

        address shortDelay = address(new TimelockCandidateFixture(24 hours - 1, true));
        vm.expectPartialRevert(IBridge.TimelockCandidateCodeHashMismatch.selector);
        bridge.rotateBaseAdminTimelock(shortDelay);

        address missingSelfAdmin = address(new TimelockCandidateFixture(24 hours, false));
        vm.expectPartialRevert(IBridge.TimelockCandidateCodeHashMismatch.selector);
        bridge.rotateBaseAdminTimelock(missingSelfAdmin);
        vm.stopPrank();
        assert(bridge.baseAdminTimelock() == BASE_ADMIN_TIMELOCK);
    }

    function testTimelockRotationChecksDelayAndSelfAdminAfterCodeHash() public {
        address valid = address(new TimelockCandidateFixture(24 hours, true));
        address shortDelay = address(new TimelockCandidateFixture(24 hours - 1, true));
        address missingSelfAdmin = address(new TimelockCandidateFixture(24 hours, false));
        Bridge fixtureBridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            valid,
            _timelockCodeHash(valid),
            PER_DEPOSIT_LIMIT,
            WINDOW_LIMIT,
            WINDOW_DURATION,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );

        vm.startPrank(valid);
        vm.expectRevert(
            abi.encodeWithSelector(IBridge.TimelockCandidateDelayTooShort.selector, shortDelay, 24 hours - 1, 24 hours)
        );
        fixtureBridge.rotateBaseAdminTimelock(shortDelay);
        vm.expectRevert(abi.encodeWithSelector(IBridge.TimelockCandidateMissingSelfAdmin.selector, missingSelfAdmin));
        fixtureBridge.rotateBaseAdminTimelock(missingSelfAdmin);
        vm.stopPrank();
        assert(fixtureBridge.baseAdminTimelock() == valid);
    }

    function testAdministrativeAuthoritiesCannotForgeMintAuthorization() public {
        _assertInvalidSignatureRejectedFrom(RUNTIME_ADMINISTRATOR);
        _assertInvalidSignatureRejectedFrom(BASE_ADMIN_TIMELOCK);
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
        bridge.rotateBridgeSigner(NEW_BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.rotateRuntimeAdministrator(NEW_RUNTIME_ADMINISTRATOR);
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBaseAdmin.selector, caller));
        bridge.rotateBaseAdminTimelock(NEW_TIMELOCK);
        vm.stopPrank();
    }

    function _assertInvalidSignatureRejectedFrom(address caller) private {
        IBridge.MintAuthorization memory authorization =
            _authorization(keccak256(abi.encode(caller, "mint")), USER, 11, SERVICE_FEE);
        vm.expectRevert(IBridge.InvalidMintAuthorizationSignature.selector);
        _submitMintAuthorization(0xDEAD, bridge, authorization, caller);
    }

    function _mintAuthorized(
        bytes32 depositId,
        address recipient,
        uint256 grossAmount,
        uint256 maximumFee,
        uint256 signerKey
    ) private {
        _mintDepositWithChargedFee(depositId, recipient, grossAmount, maximumFee, SERVICE_FEE, signerKey);
    }

    function _mintDepositWithChargedFee(
        bytes32 depositId,
        address recipient,
        uint256 grossAmount,
        uint256 maximumFee,
        uint256 chargedFee,
        uint256 signerKey
    ) private {
        _submitMintAuthorization(
            signerKey,
            bridge,
            _authorizationWithFee(depositId, recipient, grossAmount, maximumFee, chargedFee),
            address(this)
        );
    }

    function _authorization(bytes32 depositId, address recipient, uint256 grossAmount, uint256 maximumFee)
        private
        view
        returns (IBridge.MintAuthorization memory)
    {
        return _authorizationWithFee(depositId, recipient, grossAmount, maximumFee, SERVICE_FEE);
    }

    function _authorizationWithFee(
        bytes32 depositId,
        address recipient,
        uint256 grossAmount,
        uint256 maximumFee,
        uint256 chargedFee
    ) private view returns (IBridge.MintAuthorization memory) {
        return IBridge.MintAuthorization({
            depositId: depositId,
            recipient: recipient,
            grossAmount: grossAmount,
            maxServiceFee: maximumFee,
            chargedServiceFee: chargedFee,
            deadline: block.timestamp + 30 minutes,
            authorizationEpoch: bridge.mintAuthorizationEpoch()
        });
    }
}
