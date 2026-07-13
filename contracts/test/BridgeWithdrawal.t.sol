// contracts/test: verify Withdrawal burn, exclusive settlement, idempotent release, and full Base Refund.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase, Vm} from "./TestBase.sol";

contract BridgeWithdrawalTest is TestBase {
    address private constant BRIDGE_SIGNER = address(0x11);
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant BASE_ADMIN_TIMELOCK = address(0x33);
    address private constant USER = address(0x44);
    uint256 private constant MAX_SERVICE_FEE = 100;
    uint256 private constant SERVICE_FEE = 10;
    bytes32 private constant SUBACCOUNT = bytes32(uint256(0x1234));

    event WithdrawalCreated(
        uint256 indexed withdrawalId,
        address indexed requester,
        uint256 amount,
        uint256 minAmountOut,
        bytes owner,
        bytes32 subaccount
    );
    event WithdrawalReleased(
        uint256 indexed withdrawalId, uint256 amountOut, uint256 serviceFee, uint256 ledgerFee, uint256 ledgerBlockIndex
    );
    event WithdrawalRefunded(uint256 indexed withdrawalId, address indexed requester, uint256 amount);

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
            1_000,
            2_000,
            1 hours,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );
        token = bridge.bsns();
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposit(
            IBridge.DepositMintRequest(keccak256("withdrawal-funding"), USER, 1_010, SERVICE_FEE, SERVICE_FEE)
        );
    }

    function testCreateWithdrawalBurnsAndStoresEveryField() public {
        bytes memory owner = hex"010203";
        vm.expectEmit(true, true, false, true, address(bridge));
        emit WithdrawalCreated(1, USER, 600, 500, owner, SUBACCOUNT);
        uint256 withdrawalId = _createWithdrawal(600, 500, owner, SUBACCOUNT);

        assert(withdrawalId == 1);
        assert(bridge.nextWithdrawalId() == 2);
        assert(token.balanceOf(USER) == 400);
        assert(token.totalSupply() == 400);
        assert(token.allowance(USER, address(bridge)) == 0);

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.requester == USER);
        assert(withdrawal.amount == 600);
        assert(withdrawal.minAmountOut == 500);
        assert(keccak256(withdrawal.owner) == keccak256(owner));
        assert(withdrawal.subaccount == SUBACCOUNT);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Pending);
        assert(withdrawal.amountOut == 0);
        assert(withdrawal.serviceFee == 0);
        assert(withdrawal.ledgerFee == 0);
        assert(withdrawal.ledgerBlockIndex == 0);
    }

    function testCreateWithdrawalUsesSequentialIds() public {
        uint256 first = _createWithdrawal(400, 300, hex"01", bytes32(0));
        uint256 second = _createWithdrawal(300, 200, hex"02", SUBACCOUNT);
        assert(first == 1);
        assert(second == 2);
        assert(bridge.nextWithdrawalId() == 3);
        assert(bridge.getWithdrawal(first).status == IBridge.WithdrawalStatus.Pending);
        assert(bridge.getWithdrawal(second).status == IBridge.WithdrawalStatus.Pending);
    }

    function testCreateWithdrawalValidatesAmountAndMinimum() public {
        vm.startPrank(USER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, 0));
        bridge.createWithdrawal(0, 1, hex"01", bytes32(0));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMinAmountOut.selector, 0, 100));
        bridge.createWithdrawal(100, 0, hex"01", bytes32(0));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMinAmountOut.selector, 101, 100));
        bridge.createWithdrawal(100, 101, hex"01", bytes32(0));

        uint256 withdrawalId = bridge.createWithdrawal(100, 100, hex"01", bytes32(0));
        vm.stopPrank();
        assert(withdrawalId == 1);
    }

    function testCreateWithdrawalValidatesPrincipalBytes() public {
        bytes memory thirtyBytes = new bytes(30);
        bytes memory twentyNineBytes = new bytes(29);
        twentyNineBytes[28] = bytes1(0x02);

        vm.startPrank(USER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidPrincipal.selector, bytes("")));
        bridge.createWithdrawal(100, 1, bytes(""), bytes32(0));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidPrincipal.selector, thirtyBytes));
        bridge.createWithdrawal(100, 1, thirtyBytes, bytes32(0));

        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidPrincipal.selector, hex"04"));
        bridge.createWithdrawal(100, 1, hex"04", bytes32(0));

        uint256 withdrawalId = bridge.createWithdrawal(100, 1, twentyNineBytes, bytes32(0));
        vm.stopPrank();

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.owner.length == 29);
        assert(withdrawal.subaccount == bytes32(0));
    }

    function testBurnFailureRollsBackRecordAndId() public {
        vm.prank(USER);
        vm.expectRevert(
            abi.encodeWithSelector(
                bytes4(keccak256("ERC20InsufficientBalance(address,uint256,uint256)")), USER, 1_000, 1_001
            )
        );
        bridge.createWithdrawal(1_001, 1, hex"01", bytes32(0));

        assert(bridge.nextWithdrawalId() == 1);
        assert(bridge.getWithdrawal(1).status == IBridge.WithdrawalStatus.None);
        assert(token.balanceOf(USER) == 1_000);
    }

    function testGetUnknownWithdrawalReturnsNone() public view {
        IBridge.Withdrawal memory zero = bridge.getWithdrawal(0);
        IBridge.Withdrawal memory missing = bridge.getWithdrawal(type(uint256).max);
        assert(zero.status == IBridge.WithdrawalStatus.None);
        assert(missing.status == IBridge.WithdrawalStatus.None);
        assert(missing.requester == address(0));
        assert(missing.owner.length == 0);
    }

    function testAcknowledgeReleaseStoresSettlementAndAllowsDifferentCurrentFee() public {
        uint256 withdrawalId = _createWithdrawal(600, 500, hex"010203", SUBACCOUNT);
        vm.expectEmit(true, false, false, true, address(bridge));
        emit WithdrawalReleased(withdrawalId, 550, 30, 20, 0);
        _acknowledge(withdrawalId, 550, 30, 20, 0);

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Released);
        assert(withdrawal.amountOut == 550);
        assert(withdrawal.serviceFee == 30);
        assert(withdrawal.serviceFee != bridge.serviceFee());
        assert(withdrawal.ledgerFee == 20);
        assert(withdrawal.ledgerBlockIndex == 0);
        assert(token.totalSupply() == 400);
    }

    function testExactReleaseAcknowledgementIsIdempotentWithoutEvent() public {
        uint256 withdrawalId = _createWithdrawal(600, 500, hex"01", bytes32(0));
        _acknowledge(withdrawalId, 550, 30, 20, 42);

        vm.recordLogs();
        _acknowledge(withdrawalId, 550, 30, 20, 42);
        Vm.Log[] memory logs = vm.getRecordedLogs();
        assert(logs.length == 0);

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Released);
        assert(withdrawal.ledgerBlockIndex == 42);
    }

    function testReleasedAcknowledgementRejectsEveryMismatchedDetail() public {
        uint256 withdrawalId = _createWithdrawal(600, 500, hex"01", bytes32(0));
        _acknowledge(withdrawalId, 550, 30, 20, 42);

        _expectAcknowledgementMismatch(withdrawalId, 549, 30, 21, 42);
        _expectAcknowledgementMismatch(withdrawalId, 550, 29, 21, 42);
        _expectAcknowledgementMismatch(withdrawalId, 550, 30, 19, 42);
        _expectAcknowledgementMismatch(withdrawalId, 550, 30, 20, 43);
    }

    function testAcknowledgeReleaseRejectsUnauthorizedAndMissing() public {
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, address(this)));
        bridge.acknowledgeRelease(1, 1, 0, 0, 1);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.WithdrawalNotFound.selector, 1));
        bridge.acknowledgeRelease(1, 1, 0, 0, 1);
    }

    function testAcknowledgeReleaseValidatesFeeSettlementAndMinimum() public {
        uint256 feeWithdrawal = _createWithdrawal(200, 1, hex"01", bytes32(0));
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidServiceFee.selector, 101, 100));
        bridge.acknowledgeRelease(feeWithdrawal, 99, 101, 0, 1);

        uint256 mismatchWithdrawal = _createWithdrawal(200, 1, hex"02", bytes32(0));
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.SettlementAmountsMismatch.selector, 200, 150, 20, 29));
        bridge.acknowledgeRelease(mismatchWithdrawal, 150, 20, 29, 2);

        uint256 minimumWithdrawal = _createWithdrawal(200, 180, hex"03", bytes32(0));
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidMinAmountOut.selector, 180, 170));
        bridge.acknowledgeRelease(minimumWithdrawal, 170, 20, 10, 3);
    }

    function testSettlementMismatchUsesCustomErrorInsteadOfOverflowPanic() public {
        uint256 withdrawalId = _createWithdrawal(100, 1, hex"01", bytes32(0));
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(
            abi.encodeWithSelector(
                IBridge.SettlementAmountsMismatch.selector, 100, type(uint256).max, MAX_SERVICE_FEE, type(uint256).max
            )
        );
        bridge.acknowledgeRelease(
            withdrawalId, type(uint256).max, MAX_SERVICE_FEE, type(uint256).max, type(uint256).max
        );
    }

    function testLedgerBlockIndexCannotSettleTwoWithdrawals() public {
        uint256 first = _createWithdrawal(600, 500, hex"01", bytes32(0));
        uint256 second = _createWithdrawal(300, 250, hex"02", bytes32(0));
        _acknowledge(first, 550, 30, 20, 0);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.LedgerBlockAlreadyAcknowledged.selector, 0, first));
        bridge.acknowledgeRelease(second, 270, 10, 20, 0);
        assert(bridge.getWithdrawal(second).status == IBridge.WithdrawalStatus.Pending);
    }

    function testRefundRestoresFullAmountWithoutFeeOrWindowConsumption() public {
        uint256 mintedBefore = bridge.mintedInWindow();
        uint256 withdrawalId = _createWithdrawal(600, 500, hex"01", bytes32(0));
        vm.expectEmit(true, true, false, true, address(bridge));
        emit WithdrawalRefunded(withdrawalId, USER, 600);
        vm.prank(BRIDGE_SIGNER);
        bridge.refundWithdrawal(withdrawalId);

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Refunded);
        assert(withdrawal.serviceFee == 0);
        assert(withdrawal.ledgerFee == 0);
        assert(withdrawal.ledgerBlockIndex == 0);
        assert(token.balanceOf(USER) == 1_000);
        assert(token.totalSupply() == 1_000);
        assert(bridge.mintedInWindow() == mintedBefore);
    }

    function testRefundRejectsUnauthorizedMissingAndTerminalStates() public {
        uint256 refunded = _createWithdrawal(300, 200, hex"01", bytes32(0));
        vm.expectRevert(abi.encodeWithSelector(IBridge.UnauthorizedBridgeSigner.selector, address(this)));
        bridge.refundWithdrawal(refunded);

        vm.prank(BRIDGE_SIGNER);
        bridge.refundWithdrawal(refunded);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(
            abi.encodeWithSelector(
                IBridge.InvalidWithdrawalStatus.selector, refunded, IBridge.WithdrawalStatus.Refunded
            )
        );
        bridge.refundWithdrawal(refunded);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.WithdrawalNotFound.selector, 999));
        bridge.refundWithdrawal(999);

        uint256 released = _createWithdrawal(300, 200, hex"02", bytes32(0));
        _acknowledge(released, 250, 30, 20, 77);
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(
            abi.encodeWithSelector(
                IBridge.InvalidWithdrawalStatus.selector, released, IBridge.WithdrawalStatus.Released
            )
        );
        bridge.refundWithdrawal(released);
    }

    function testRefundedWithdrawalCannotBeAcknowledged() public {
        uint256 withdrawalId = _createWithdrawal(600, 500, hex"01", bytes32(0));
        vm.prank(BRIDGE_SIGNER);
        bridge.refundWithdrawal(withdrawalId);

        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(
            abi.encodeWithSelector(
                IBridge.InvalidWithdrawalStatus.selector, withdrawalId, IBridge.WithdrawalStatus.Refunded
            )
        );
        bridge.acknowledgeRelease(withdrawalId, 550, 30, 20, 42);
    }

    function testBridgeExposureAcrossPendingReleaseAndRefund() public {
        uint256 first = _createWithdrawal(600, 500, hex"01", bytes32(0));
        uint256 pendingExposure = token.totalSupply() + bridge.getWithdrawal(first).amount;
        assert(pendingExposure == 1_000);

        _acknowledge(first, 550, 30, 20, 42);
        assert(token.totalSupply() == 400);

        uint256 second = _createWithdrawal(300, 200, hex"02", bytes32(0));
        uint256 exposureBeforeRefund = token.totalSupply() + bridge.getWithdrawal(second).amount;
        assert(exposureBeforeRefund == 400);
        vm.prank(BRIDGE_SIGNER);
        bridge.refundWithdrawal(second);
        assert(token.totalSupply() == 400);
    }

    function testFuzzValidSettlementPartition(
        uint256 amountSeed,
        uint256 amountOutSeed,
        uint256 feeSeed,
        uint256 minimumSeed,
        uint256 ledgerBlockIndex
    ) public {
        uint256 amount = (amountSeed % 1_000) + 1;
        uint256 amountOut = (amountOutSeed % amount) + 1;
        uint256 remaining = amount - amountOut;
        uint256 feeLimit = remaining < MAX_SERVICE_FEE ? remaining : MAX_SERVICE_FEE;
        uint256 withdrawalServiceFee = feeSeed % (feeLimit + 1);
        uint256 ledgerFee = remaining - withdrawalServiceFee;
        uint256 minAmountOut = (minimumSeed % amountOut) + 1;

        uint256 withdrawalId = _createWithdrawal(amount, minAmountOut, hex"01", bytes32(0));
        _acknowledge(withdrawalId, amountOut, withdrawalServiceFee, ledgerFee, ledgerBlockIndex);
        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Released);
        assert(withdrawal.amountOut + withdrawal.serviceFee + withdrawal.ledgerFee == amount);
        assert(withdrawal.amountOut >= withdrawal.minAmountOut);
        assert(withdrawal.serviceFee <= MAX_SERVICE_FEE);
    }

    function _createWithdrawal(uint256 amount, uint256 minAmountOut, bytes memory owner, bytes32 subaccount)
        private
        returns (uint256 withdrawalId)
    {
        vm.prank(USER);
        return bridge.createWithdrawal(amount, minAmountOut, owner, subaccount);
    }

    function _acknowledge(
        uint256 withdrawalId,
        uint256 amountOut,
        uint256 withdrawalServiceFee,
        uint256 ledgerFee,
        uint256 ledgerBlockIndex
    ) private {
        vm.prank(BRIDGE_SIGNER);
        bridge.acknowledgeRelease(withdrawalId, amountOut, withdrawalServiceFee, ledgerFee, ledgerBlockIndex);
    }

    function _expectAcknowledgementMismatch(
        uint256 withdrawalId,
        uint256 amountOut,
        uint256 withdrawalServiceFee,
        uint256 ledgerFee,
        uint256 ledgerBlockIndex
    ) private {
        vm.prank(BRIDGE_SIGNER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.ReleaseAcknowledgementMismatch.selector, withdrawalId));
        bridge.acknowledgeRelease(withdrawalId, amountOut, withdrawalServiceFee, ledgerFee, ledgerBlockIndex);
    }
}
