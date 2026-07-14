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
    address private constant BASE_ADMIN_TIMELOCK = address(0x33);
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

    function testConstructorRejectsZeroAndDuplicateRoles() public {
        vm.expectRevert(IBridge.ZeroAddress.selector);
        new Bridge("kinic", "KINIC", 8, address(0), RUNTIME_ADMINISTRATOR, BASE_ADMIN_TIMELOCK, 1, 1, 1, 1, 0);

        vm.expectRevert(IBridge.RoleAddressesMustDiffer.selector);
        new Bridge("kinic", "KINIC", 8, BRIDGE_SIGNER, BRIDGE_SIGNER, BASE_ADMIN_TIMELOCK, 1, 1, 1, 1, 0);
    }

    function testConstructorRejectsZeroLimitsAndFeeAboveMaximum() public {
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deploy(0, WINDOW_LIMIT, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deploy(PER_DEPOSIT_LIMIT, 0, WINDOW_DURATION, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deploy(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, 0, MAX_SERVICE_FEE, SERVICE_FEE);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        _deploy(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, 0, 0);

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        _deploy(PER_DEPOSIT_LIMIT, WINDOW_LIMIT, WINDOW_DURATION, 100, 101);
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
        vm.startPrank(BRIDGE_SIGNER);

        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.mintDeposit(_request(keccak256("zero-recipient"), address(0), 110, 10));

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

    function testBatchMintsInInputOrder() public {
        IBridge.DepositMintRequest[] memory requests = new IBridge.DepositMintRequest[](2);
        requests[0] = _request(keccak256("batch-0"), RECIPIENT, 110, 10);
        requests[1] = _request(keccak256("batch-1"), address(0x55), 210, 10);

        vm.expectEmit(true, true, false, true, address(bridge));
        emit DepositMinted(requests[0].depositId, RECIPIENT, 110, 10, 100);
        vm.expectEmit(true, true, false, true, address(bridge));
        emit DepositMinted(requests[1].depositId, address(0x55), 210, 10, 200);
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposits(requests);

        assert(token.balanceOf(RECIPIENT) == 100);
        assert(token.balanceOf(address(0x55)) == 200);
        assert(bridge.mintedInWindow() == 300);
    }

    function testBatchRejectsEmptyAndDuplicateAtomically() public {
        IBridge.DepositMintRequest[] memory empty = new IBridge.DepositMintRequest[](0);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(IBridge.EmptyBatch.selector);
        bridge.mintDeposits(empty);

        bytes32 duplicateId = keccak256("batch-duplicate");
        IBridge.DepositMintRequest[] memory requests = new IBridge.DepositMintRequest[](2);
        requests[0] = _request(duplicateId, RECIPIENT, 110, 10);
        requests[1] = _request(duplicateId, address(0x55), 210, 10);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.DepositAlreadyProcessed.selector, duplicateId));
        bridge.mintDeposits(requests);

        assert(!bridge.isDepositProcessed(duplicateId));
        assert(token.totalSupply() == 0);
        assert(bridge.mintedInWindow() == 0);
    }

    function testBatchRejectsMoreThanFourItems() public {
        IBridge.DepositMintRequest[] memory requests = new IBridge.DepositMintRequest[](5);
        for (uint256 index = 0; index < requests.length; ++index) {
            requests[index] = _request(bytes32(index + 1), RECIPIENT, 110, 10);
        }
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.BatchTooLarge.selector, 5, 4));
        bridge.mintDeposits(requests);
        assert(token.totalSupply() == 0);
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

    function testBatchInvalidLaterItemRollsBackEarlierMark() public {
        IBridge.DepositMintRequest[] memory requests = new IBridge.DepositMintRequest[](2);
        requests[0] = _request(keccak256("valid-first"), RECIPIENT, 110, 10);
        requests[1] = _request(keccak256("invalid-second"), address(0), 110, 10);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(IBridge.ZeroAddress.selector);
        bridge.mintDeposits(requests);

        assert(!bridge.isDepositProcessed(requests[0].depositId));
        assert(!bridge.isDepositProcessed(requests[1].depositId));
        assert(token.totalSupply() == 0);
        assert(bridge.mintedInWindow() == 0);
    }

    function testBatchWindowViolationRollsBackEveryDeposit() public {
        IBridge.DepositMintRequest[] memory requests = new IBridge.DepositMintRequest[](3);
        requests[0] = _request(keccak256("window-0"), RECIPIENT, 1_010, 10);
        requests[1] = _request(keccak256("window-1"), RECIPIENT, 1_010, 10);
        requests[2] = _request(keccak256("window-2"), RECIPIENT, 11, 10);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.MintWindowLimitExceeded.selector, 2_001, 2_000));
        bridge.mintDeposits(requests);

        assert(!bridge.isDepositProcessed(requests[0].depositId));
        assert(!bridge.isDepositProcessed(requests[1].depositId));
        assert(!bridge.isDepositProcessed(requests[2].depositId));
        assert(token.totalSupply() == 0);
        assert(bridge.mintedInWindow() == 0);
    }

    function testMultipleBatchesAccumulateInOneWindow() public {
        IBridge.DepositMintRequest[] memory first = new IBridge.DepositMintRequest[](2);
        first[0] = _request(keccak256("first-0"), RECIPIENT, 510, 10);
        first[1] = _request(keccak256("first-1"), RECIPIENT, 510, 10);
        IBridge.DepositMintRequest[] memory second = new IBridge.DepositMintRequest[](2);
        second[0] = _request(keccak256("second-0"), RECIPIENT, 510, 10);
        second[1] = _request(keccak256("second-1"), RECIPIENT, 510, 10);

        vm.startPrank(BRIDGE_SIGNER);
        bridge.mintDeposits(first);
        bridge.mintDeposits(second);
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
        return new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
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
