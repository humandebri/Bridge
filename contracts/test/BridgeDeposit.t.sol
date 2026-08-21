// contracts/test: verify Deposit mint authorization, atomic deduplication, fee deduction, and fixed-window limits.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";

contract BridgeDepositTest is TestBase {
    uint256 private constant BRIDGE_SIGNER_KEY = 0xA11CE;
    address private BRIDGE_SIGNER;
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private BASE_ADMIN_TIMELOCK;
    address private constant RECIPIENT = address(0x44);
    uint256 private constant PER_DEPOSIT_LIMIT = 1_000;
    uint256 private constant WINDOW_LIMIT = 2_000;
    uint64 private constant WINDOW_DURATION = 1 hours;
    uint256 private constant MAX_SERVICE_FEE = 100;
    uint256 private constant SERVICE_FEE = 10;

    event DepositMinted(
        bytes32 indexed depositId,
        address indexed recipient,
        bytes32 indexed authorizationDigest,
        uint256 grossAmount,
        uint256 serviceFee,
        uint256 mintedAmount
    );

    Bridge private bridge;
    IBSNS private token;

    function setUp() public {
        BRIDGE_SIGNER = vm.addr(BRIDGE_SIGNER_KEY);
        BASE_ADMIN_TIMELOCK = _deployTestTimelock(address(0x33));
        vm.warp(1_000_000);
        bridge = _deploy(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);
        token = bridge.bsns();
    }

    function testConstructorInitializesRolesLimitsAndBoundToken() public view {
        assert(bridge.bridgeSigner() == BRIDGE_SIGNER);
        assert(bridge.mintAuthorizationEpoch() == 2);
        assert(bridge.runtimeAdministrator() == RUNTIME_ADMINISTRATOR);
        assert(bridge.baseAdminTimelock() == BASE_ADMIN_TIMELOCK);
        assert(bridge.serviceFee() == SERVICE_FEE);
        assert(bridge.MAX_SERVICE_FEE() == MAX_SERVICE_FEE);
        assert(bridge.perDepositLimit() == PER_DEPOSIT_LIMIT);
        assert(bridge.mintWindowLimit() == WINDOW_LIMIT);
        assert(bridge.mintWindowDuration() == WINDOW_DURATION);
        // The test fixes block time before deployment and checks that exact constructor anchor.
        // forge-lint: disable-next-line(block-timestamp)
        assert(bridge.mintWindowStartedAt() == block.timestamp);
        assert(bridge.mintedInWindow() == 0);
        assert(!bridge.depositMintsPaused());
        assert(!bridge.withdrawalsPaused());
        assert(bridge.nextWithdrawalId() == 1);
        assert(token.bridge() == address(bridge));
        assert(_sameString(token.name(), "KINIC"));
        assert(_sameString(token.symbol(), "KINIC"));
        assert(token.decimals() == 8);
    }

    function testConstructorStartsBothAssetFlowsPaused() public {
        Bridge freshBridge = _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);
        assert(freshBridge.depositMintsPaused());
        assert(freshBridge.withdrawalsPaused());
    }

    function testConstructorRejectsZeroAndDuplicateRoles() public {
        vm.expectRevert(IBridge.ZeroAddress.selector);
        new Bridge(
            address(0),
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            1,
            1,
            1,
            1,
            0
        );

        vm.expectRevert(IBridge.RoleAddressesMustDiffer.selector);
        new Bridge(
            BRIDGE_SIGNER, BRIDGE_SIGNER, BASE_ADMIN_TIMELOCK, _timelockCodeHash(BASE_ADMIN_TIMELOCK), 1, 1, 1, 1, 0
        );
    }

    function testConstructorRejectsZeroLimitsAndFeeAboveMaximum() public {
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deployRaw(0, WINDOW_LIMIT, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deployRaw(PER_DEPOSIT_LIMIT, 0, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, 0, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, 0, 0);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, 100, 101);
    }

    function testConstructorRejectsValuesOutsideCanisterAndWindowBounds() public {
        vm.expectRevert(abi.encodeWithSelector(IBridge.ValueExceedsU128.selector, uint256(type(uint128).max) + 1));
        _deployRaw(uint256(type(uint128).max) + 1, WINDOW_LIMIT, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.ValueExceedsU128.selector, uint256(type(uint128).max) + 1));
        _deployRaw(PER_DEPOSIT_LIMIT, uint256(type(uint128).max) + 1, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.ValueExceedsU128.selector, uint256(type(uint128).max) + 1));
        _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, uint256(type(uint128).max) + 1, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMintWindowDuration.selector, 1, 1 hours, 30 days));
        _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, 1, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(
            abi.encodeWithSelector(IBridge.InvalidMintWindowDuration.selector, 30 days + 1, 1 hours, 30 days)
        );
        _deployRaw(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, 30 days + 1, MAX_SERVICE_FEE, SERVICE_FEE);
    }

    function testAuthorizedMintDeductsFeeAndMarksOpaqueId() public {
        bytes32 depositId = bytes32(0);
        IBridge.MintAuthorization memory authorization = _authorization(depositId, RECIPIENT, 110, 10);
        bytes32 digest = _mintAuthorizationDigest(address(bridge), authorization);

        vm.expectEmit(true, true, true, true, address(bridge));
        emit DepositMinted(depositId, RECIPIENT, digest, 110, 10, 100);
        _submit(bridge, authorization);

        assert(token.balanceOf(RECIPIENT) == 100);
        assert(token.totalSupply() == 100);
        assert(bridge.mintedInWindow() == 100);
        assert(bridge.isDepositProcessed(depositId));
    }

    function testAuthorizedMintAcceptsAnyCallerButRejectsWrongSignature() public {
        IBridge.MintAuthorization memory authorization = _authorization(keccak256("permissionless"), RECIPIENT, 110, 10);
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, bridge, authorization, address(0xBEEF));
        assert(token.balanceOf(RECIPIENT) == 100);

        IBridge.MintAuthorization memory invalid = _authorization(keccak256("invalid"), RECIPIENT, 110, 10);
        vm.expectRevert(IBridge.InvalidMintAuthorizationSignature.selector);
        _submitMintAuthorization(0xB0B, bridge, invalid, address(this));
    }

    function testAuthorizedMintRejectsInvalidRequestFields() public {
        address tokenAddress = address(bridge.bsns());
        vm.expectRevert(IBridge.ZeroAddress.selector);
        _submit(bridge, _authorization(keccak256("zero-recipient"), address(0), 110, 10));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMintRecipient.selector, address(bridge)));
        _submit(bridge, _authorization(keccak256("bridge-recipient"), address(bridge), 110, 10));
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMintRecipient.selector, tokenAddress));
        _submit(bridge, _authorization(keccak256("token-recipient"), tokenAddress, 110, 10));

        vm.expectRevert(abi.encodeWithSelector(IBridge.ServiceFeeExceedsUserMaximum.selector, 10, 9));
        _submit(bridge, _authorization(keccak256("fee-maximum"), RECIPIENT, 110, 9));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        _submit(bridge, _authorizationWithFee(keccak256("fee-cap"), RECIPIENT, 110, 101, 101));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 10));
        _submit(bridge, _authorization(keccak256("zero-net"), RECIPIENT, 10, 10));

        vm.expectRevert(abi.encodeWithSelector(IBridge.DepositMintLimitExceeded.selector, 1_001, 1_000));
        _submit(bridge, _authorization(keccak256("per-deposit"), RECIPIENT, 1_011, 10));
    }

    function testAcceptedFeeRemainsFixedAfterRuntimeFeeChanges() public {
        IBridge.MintAuthorization memory authorization = _authorization(keccak256("fixed-fee"), RECIPIENT, 110, 10);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(20);

        bytes32 digest = _mintAuthorizationDigest(address(bridge), authorization);
        vm.expectEmit(true, true, true, true, address(bridge));
        emit DepositMinted(authorization.depositId, RECIPIENT, digest, 110, 10, 100);
        _submit(bridge, authorization);

        assert(token.balanceOf(RECIPIENT) == 100);
        assert(bridge.serviceFee() == 20);
    }

    function testAuthorizedMintRejectsProcessedIdWithoutChangingSupply() public {
        bytes32 depositId = keccak256("duplicate");
        IBridge.MintAuthorization memory authorization = _authorization(depositId, RECIPIENT, 110, 10);
        _submit(bridge, authorization);
        vm.expectRevert(abi.encodeWithSelector(IBridge.DepositAlreadyProcessed.selector, depositId));
        _submit(bridge, authorization);
        assert(token.totalSupply() == 100);
        assert(bridge.mintedInWindow() == 100);
    }

    function testMintAuthorizationDeadlineIsInclusiveAndThenExpires() public {
        IBridge.MintAuthorization memory atDeadline = _authorization(keccak256("at-deadline"), RECIPIENT, 110, 10);
        vm.warp(atDeadline.deadline);
        _submit(bridge, atDeadline);

        IBridge.MintAuthorization memory expired = _authorization(keccak256("expired"), RECIPIENT, 110, 10);
        vm.warp(expired.deadline + 1);
        vm.expectRevert(
            abi.encodeWithSelector(IBridge.MintAuthorizationExpired.selector, expired.deadline + 1, expired.deadline)
        );
        _submit(bridge, expired);
    }

    function testMintAuthorizationRejectsAnySignedFieldMutation() public {
        IBridge.MintAuthorization memory authorization =
            _authorization(keccak256("immutable-fields"), RECIPIENT, 110, 10);
        bytes memory signature = _signMintAuthorization(BRIDGE_SIGNER_KEY, bridge, authorization);
        authorization.recipient = address(0xBEEF);

        vm.expectRevert(IBridge.InvalidMintAuthorizationSignature.selector);
        bridge.mintDepositWithAuthorization(authorization, signature);
    }

    function testMintAuthorizationRejectsMalformedAndHighSSignatures() public {
        IBridge.MintAuthorization memory malformed =
            _authorization(keccak256("malformed-signature"), RECIPIENT, 110, 10);
        vm.expectRevert(IBridge.InvalidMintAuthorizationSignature.selector);
        bridge.mintDepositWithAuthorization(malformed, hex"0102");

        IBridge.MintAuthorization memory malleable = _authorization(keccak256("high-s-signature"), RECIPIENT, 110, 10);
        (uint8 v, bytes32 r, bytes32 s) =
            vm.sign(BRIDGE_SIGNER_KEY, _mintAuthorizationDigest(address(bridge), malleable));
        uint256 secp256k1Order = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;
        bytes32 highS = bytes32(secp256k1Order - uint256(s));
        vm.expectRevert(IBridge.InvalidMintAuthorizationSignature.selector);
        bridge.mintDepositWithAuthorization(malleable, abi.encodePacked(r, highS, v));
    }

    function testPauseInvalidatesOutstandingAuthorizationByEpoch() public {
        IBridge.MintAuthorization memory authorization = _authorization(keccak256("old-epoch"), RECIPIENT, 110, 10);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseDepositMints();
        assert(bridge.mintAuthorizationEpoch() == 3);

        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseDepositMints();
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintAuthorizationEpochMismatch.selector, 2, 4));
        _submit(bridge, authorization);
    }

    function testBridgeSnapshotReturnsOneConsistentView() public view {
        uint256 expectedBlockTimestamp = block.timestamp;
        IBridge.BridgeSnapshot memory snapshot = bridge.bridgeSnapshot();
        assert(snapshot.blockNumber == block.number);
        assert(snapshot.blockTimestamp == expectedBlockTimestamp);
        assert(snapshot.bridgeSigner == BRIDGE_SIGNER);
        assert(snapshot.mintAuthorizationEpoch == 2);
        assert(snapshot.serviceFee == SERVICE_FEE);
        assert(snapshot.maxServiceFee == MAX_SERVICE_FEE);
        assert(snapshot.perDepositLimit == PER_DEPOSIT_LIMIT);
        assert(snapshot.mintWindowLimit == WINDOW_LIMIT);
        assert(snapshot.mintWindowDuration == WINDOW_DURATION);
        assert(snapshot.mintWindowStartedAt == bridge.mintWindowStartedAt());
        assert(snapshot.mintedInWindow == 0);
        assert(!snapshot.depositMintsPaused);
        assert(!snapshot.withdrawalsPaused);
    }

    function testMultipleDepositsAccumulateInOneWindow() public {
        _submit(bridge, _authorization(keccak256("deposit-0"), RECIPIENT, 510, 10));
        _submit(bridge, _authorization(keccak256("deposit-1"), RECIPIENT, 510, 10));
        _submit(bridge, _authorization(keccak256("deposit-2"), RECIPIENT, 510, 10));
        _submit(bridge, _authorization(keccak256("deposit-3"), RECIPIENT, 510, 10));
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintWindowLimitExceeded.selector, 1, 0));
        _submit(bridge, _authorization(keccak256("over-window"), RECIPIENT, 11, 10));

        assert(bridge.mintedInWindow() == 2_000);
        assert(token.balanceOf(RECIPIENT) == 2_000);
    }

    function testFixedWindowResetsAtExactBoundary() public {
        Bridge limitedBridge = _deploy(1_000, 1_000, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);
        uint256 startedAt = limitedBridge.mintWindowStartedAt();
        _submit(limitedBridge, _authorization(keccak256("window-full"), RECIPIENT, 1_010, 10));

        vm.warp(startedAt + WINDOW_DURATION - 1);
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintWindowLimitExceeded.selector, 1, 0));
        IBridge.MintAuthorization memory beforeBoundary =
            _authorization(keccak256("before-boundary"), RECIPIENT, 11, 10);
        beforeBoundary.deadline = startedAt + WINDOW_DURATION + 30 minutes;
        _submit(limitedBridge, beforeBoundary);

        vm.warp(startedAt + WINDOW_DURATION);
        IBridge.MintAuthorization memory atBoundary = _authorization(keccak256("at-boundary"), RECIPIENT, 1_010, 10);
        atBoundary.deadline = startedAt + WINDOW_DURATION + 30 minutes;
        _submit(limitedBridge, atBoundary);
        assert(limitedBridge.mintWindowStartedAt() == startedAt + WINDOW_DURATION);
        assert(limitedBridge.mintedInWindow() == 1_000);
        assert(limitedBridge.bsns().balanceOf(RECIPIENT) == 2_000);
    }

    function testWindowAfterIdleAnchorsAtFirstSuccessfulMint() public {
        uint256 startedAt = bridge.mintWindowStartedAt();
        vm.warp(startedAt + (5 * WINDOW_DURATION));
        _submit(bridge, _authorization(keccak256("after-idle"), RECIPIENT, 110, 10));
        // The successful mint must anchor the new fixed window at this test-controlled timestamp.
        // forge-lint: disable-next-line(block-timestamp)
        assert(bridge.mintWindowStartedAt() == block.timestamp);
        assert(bridge.mintedInWindow() == 100);
    }

    function testFuzzValidSingleMint(uint256 grossAmount) public {
        grossAmount = (grossAmount % PER_DEPOSIT_LIMIT) + SERVICE_FEE + 1;
        bytes32 depositId = keccak256(abi.encode(grossAmount));
        _submit(bridge, _authorization(depositId, RECIPIENT, grossAmount, SERVICE_FEE));
        uint256 expectedMint = grossAmount - SERVICE_FEE;
        assert(token.balanceOf(RECIPIENT) == expectedMint);
        assert(bridge.mintedInWindow() == expectedMint);
    }

    function _deploy(
        uint256 perDepositLimit,
        uint256 windowLimit,
        uint64 windowDuration,
        uint256 maxServiceFee,
        uint256 initialServiceFee
    ) private returns (Bridge) {
        Bridge deployed = _deployRaw(perDepositLimit, windowLimit, windowDuration, maxServiceFee, initialServiceFee);
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        deployed.unpauseDepositMints();
        deployed.unpauseWithdrawals();
        vm.stopPrank();
        return deployed;
    }

    function _deployRaw(
        uint256 perDepositLimit,
        uint256 windowLimit,
        uint64 windowDuration,
        uint256 maxServiceFee,
        uint256 initialServiceFee
    ) private returns (Bridge) {
        return new Bridge(
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            perDepositLimit,
            windowLimit,
            windowDuration,
            maxServiceFee,
            initialServiceFee
        );
    }

    function _authorization(bytes32 depositId, address recipient, uint256 grossAmount, uint256 maximumServiceFee)
        private
        view
        returns (IBridge.MintAuthorization memory)
    {
        return _authorizationWithFee(depositId, recipient, grossAmount, maximumServiceFee, SERVICE_FEE);
    }

    function _authorizationWithFee(
        bytes32 depositId,
        address recipient,
        uint256 grossAmount,
        uint256 maximumServiceFee,
        uint256 chargedServiceFee
    ) private view returns (IBridge.MintAuthorization memory) {
        return IBridge.MintAuthorization({
            depositId: depositId,
            recipient: recipient,
            grossAmount: grossAmount,
            maxServiceFee: maximumServiceFee,
            chargedServiceFee: chargedServiceFee,
            deadline: block.timestamp + 30 minutes,
            authorizationEpoch: 2
        });
    }

    function _submit(Bridge target, IBridge.MintAuthorization memory authorization) private {
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, target, authorization, address(this));
    }
}
