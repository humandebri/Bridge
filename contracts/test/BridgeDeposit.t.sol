// contracts/test: verify Deposit mint authorization, atomic deduplication, fee deduction, and fixed-window limits.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";

contract BridgeDepositTest is TestBase {
    address private constant BRIDGE_SIGNER = address(0x11);
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
        uint256 grossAmount,
        uint256 serviceFee,
        uint256 mintedAmount
    );

    Bridge private bridge;
    IBSNS private token;

    function setUp() public {
        BASE_ADMIN_TIMELOCK = _deployTestTimelock(address(0x33));
        vm.warp(1_000_000);
        bridge = _deploy(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);
        token = bridge.bsns();
    }

    function testConstructorInitializesRolesLimitsAndBoundToken() public view {
        assert(bridge.bridgeSigner() == BRIDGE_SIGNER);
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
        assert(_sameString(token.name(), "kinic"));
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
            "kinic",
            "KINIC",
            8,
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
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            BRIDGE_SIGNER,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            1,
            1,
            1,
            1,
            0
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

    function testMintDepositDeductsFeeAndMarksOpaqueId() public {
        bytes32 depositId = bytes32(0);
        IBridge.DepositMintRequest memory request = _request(depositId, RECIPIENT, 110, 10);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit DepositMinted(depositId, RECIPIENT, 110, 10, 100);
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposit(request);

        assert(token.balanceOf(RECIPIENT) == 100);
        assert(token.totalSupply() == 100);
        assert(bridge.mintedInWindow() == 100);
        assert(bridge.isDepositProcessed(depositId));
    }

    function testMintDepositRejectsUnauthorizedCaller() public {
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, address(this)));
        bridge.mintDeposit(_request(keccak256("unauthorized"), RECIPIENT, 110, 10));
    }

    function testMintDepositRejectsInvalidRequestFields() public {
        address tokenAddress = address(bridge.bsns());
        vm.startPrank(BRIDGE_SIGNER);

        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.mintDeposit(_request(keccak256("zero-recipient"), address(0), 110, 10));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMintRecipient.selector, address(bridge)));
        bridge.mintDeposit(_request(keccak256("bridge-recipient"), address(bridge), 110, 10));
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMintRecipient.selector, tokenAddress));
        bridge.mintDeposit(_request(keccak256("token-recipient"), tokenAddress, 110, 10));

        vm.expectRevert(abi.encodeWithSelector(IBridge.ServiceFeeExceedsUserMaximum.selector, 10, 9));
        bridge.mintDeposit(_request(keccak256("fee-maximum"), RECIPIENT, 110, 9));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        bridge.mintDeposit(IBridge.DepositMintRequest(keccak256("fee-cap"), RECIPIENT, 110, 101, 101));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 10));
        bridge.mintDeposit(_request(keccak256("zero-net"), RECIPIENT, 10, 10));

        vm.expectRevert(abi.encodeWithSelector(IBridge.DepositMintLimitExceeded.selector, 1_001, 1_000));
        bridge.mintDeposit(_request(keccak256("per-deposit"), RECIPIENT, 1_011, 10));
        vm.stopPrank();
    }

    function testAcceptedFeeRemainsFixedAfterRuntimeFeeChanges() public {
        IBridge.DepositMintRequest memory request = _request(keccak256("fixed-fee"), RECIPIENT, 110, 10);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(20);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit DepositMinted(request.depositId, RECIPIENT, 110, 10, 100);
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposit(request);

        assert(token.balanceOf(RECIPIENT) == 100);
        assert(bridge.serviceFee() == 20);
    }

    function testMintDepositRejectsProcessedIdWithoutChangingSupply() public {
        bytes32 depositId = keccak256("duplicate");
        IBridge.DepositMintRequest memory request = _request(depositId, RECIPIENT, 110, 10);
        vm.startPrank(BRIDGE_SIGNER);
        bridge.mintDeposit(request);
        vm.expectRevert(abi.encodeWithSelector(IBridge.DepositAlreadyProcessed.selector, depositId));
        bridge.mintDeposit(request);
        vm.stopPrank();
        assert(token.totalSupply() == 100);
        assert(bridge.mintedInWindow() == 100);
    }

    function testBridgeSnapshotReturnsOneConsistentView() public view {
        uint256 expectedBlockTimestamp = block.timestamp;
        IBridge.BridgeSnapshot memory snapshot = bridge.bridgeSnapshot();
        assert(snapshot.blockNumber == block.number);
        assert(snapshot.blockTimestamp == expectedBlockTimestamp);
        assert(snapshot.bridgeSigner == BRIDGE_SIGNER);
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
        vm.startPrank(BRIDGE_SIGNER);
        bridge.mintDeposit(_request(keccak256("deposit-0"), RECIPIENT, 510, 10));
        bridge.mintDeposit(_request(keccak256("deposit-1"), RECIPIENT, 510, 10));
        bridge.mintDeposit(_request(keccak256("deposit-2"), RECIPIENT, 510, 10));
        bridge.mintDeposit(_request(keccak256("deposit-3"), RECIPIENT, 510, 10));
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintWindowLimitExceeded.selector, 1, 0));
        bridge.mintDeposit(_request(keccak256("over-window"), RECIPIENT, 11, 10));
        vm.stopPrank();

        assert(bridge.mintedInWindow() == 2_000);
        assert(token.balanceOf(RECIPIENT) == 2_000);
    }

    function testFixedWindowResetsAtExactBoundary() public {
        Bridge limitedBridge = _deploy(1_000, 1_000, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);
        uint256 startedAt = limitedBridge.mintWindowStartedAt();
        vm.prank(BRIDGE_SIGNER);
        limitedBridge.mintDeposit(_request(keccak256("window-full"), RECIPIENT, 1_010, 10));

        vm.warp(startedAt + WINDOW_DURATION - 1);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintWindowLimitExceeded.selector, 1, 0));
        limitedBridge.mintDeposit(_request(keccak256("before-boundary"), RECIPIENT, 11, 10));

        vm.warp(startedAt + WINDOW_DURATION);
        vm.prank(BRIDGE_SIGNER);
        limitedBridge.mintDeposit(_request(keccak256("at-boundary"), RECIPIENT, 1_010, 10));
        assert(limitedBridge.mintWindowStartedAt() == startedAt + WINDOW_DURATION);
        assert(limitedBridge.mintedInWindow() == 1_000);
        assert(limitedBridge.bsns().balanceOf(RECIPIENT) == 2_000);
    }

    function testWindowAfterIdleAnchorsAtFirstSuccessfulMint() public {
        uint256 startedAt = bridge.mintWindowStartedAt();
        vm.warp(startedAt + (5 * WINDOW_DURATION));
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposit(_request(keccak256("after-idle"), RECIPIENT, 110, 10));
        // The successful mint must anchor the new fixed window at this test-controlled timestamp.
        // forge-lint: disable-next-line(block-timestamp)
        assert(bridge.mintWindowStartedAt() == block.timestamp);
        assert(bridge.mintedInWindow() == 100);
    }

    function testFuzzValidSingleMint(uint256 grossAmount) public {
        grossAmount = (grossAmount % PER_DEPOSIT_LIMIT) + SERVICE_FEE + 1;
        bytes32 depositId = keccak256(abi.encode(grossAmount));
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposit(_request(depositId, RECIPIENT, grossAmount, SERVICE_FEE));
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
            "kinic",
            "KINIC",
            8,
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

    function _request(bytes32 depositId, address recipient, uint256 grossAmount, uint256 maximumServiceFee)
        private
        pure
        returns (IBridge.DepositMintRequest memory)
    {
        return IBridge.DepositMintRequest(depositId, recipient, grossAmount, maximumServiceFee, SERVICE_FEE);
    }
}
