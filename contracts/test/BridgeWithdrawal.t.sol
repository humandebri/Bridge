// contracts/test: verify irreversible Withdrawal commitment and deterministic ICP payout quotes.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";

contract WithdrawalBatcher {
    function createTwice(IBridge bridge, IBSNS token) external {
        token.approve(address(bridge), 700);
        bridge.createWithdrawal(400, 100, hex"01", bytes32(0));
        bridge.createWithdrawal(300, 100, hex"02", bytes32(0));
    }
}

contract BridgeWithdrawalTest is TestBase {
    uint256 private constant BRIDGE_SIGNER_KEY = 0xA11CE;
    address private BRIDGE_SIGNER;
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private BASE_ADMIN_TIMELOCK;
    address private constant USER = address(0x44);
    uint256 private constant MAX_SERVICE_FEE = 100;
    uint256 private constant SERVICE_FEE = 10;
    bytes32 private constant SUBACCOUNT = bytes32(uint256(0x1234));

    event WithdrawalCommitted(
        uint256 indexed withdrawalId,
        address indexed requester,
        uint256 amount,
        uint256 maxServiceFee,
        uint256 chargedServiceFee,
        uint256 amountOut,
        bytes owner,
        bytes32 subaccount
    );

    Bridge private bridge;
    IBSNS private token;

    function setUp() public {
        BRIDGE_SIGNER = vm.addr(BRIDGE_SIGNER_KEY);
        BASE_ADMIN_TIMELOCK = _deployTestTimelock(address(0x33));
        bridge = new Bridge(
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            1_000,
            2_000,
            1 hours,
            MAX_SERVICE_FEE,
            SERVICE_FEE
        );
        token = bridge.bsns();
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseDepositMints();
        vm.prank(BASE_ADMIN_TIMELOCK);
        bridge.unpauseWithdrawals();
        _mint(keccak256("withdrawal-funding"), USER, 1_010);
    }

    function testCreateWithdrawalBurnsAndCommitsDeterministicPayout() public {
        bytes memory owner = hex"010203";
        vm.prank(USER);
        token.approve(address(bridge), 600);
        vm.expectEmit(true, true, false, true, address(bridge));
        emit WithdrawalCommitted(1, USER, 600, SERVICE_FEE, SERVICE_FEE, 590, owner, SUBACCOUNT);
        vm.prank(USER);
        uint256 withdrawalId = bridge.createWithdrawal(600, SERVICE_FEE, owner, SUBACCOUNT);

        assert(withdrawalId == 1);
        assert(bridge.nextWithdrawalId() == 2);
        assert(token.balanceOf(USER) == 400);
        assert(token.totalSupply() == 400);
        assert(token.allowance(USER, address(bridge)) == 0);

        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
        assert(withdrawal.requester == USER);
        assert(withdrawal.amount == 600);
        assert(withdrawal.maxServiceFee == SERVICE_FEE);
        assert(withdrawal.chargedServiceFee == SERVICE_FEE);
        assert(withdrawal.amountOut == 590);
        assert(keccak256(withdrawal.owner) == keccak256(owner));
        assert(withdrawal.subaccount == SUBACCOUNT);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Committed);
    }

    function testFeeDriftRevertsBeforeBurn() public {
        vm.prank(USER);
        token.approve(address(bridge), 100);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(11);

        vm.prank(USER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.ServiceFeeExceedsUserMaximum.selector, 11, 10));
        bridge.createWithdrawal(100, 10, hex"01", bytes32(0));

        assert(token.balanceOf(USER) == 1_000);
        assert(token.totalSupply() == 1_000);
        assert(bridge.nextWithdrawalId() == 1);
    }

    function testAmountMustExceedChargedFee() public {
        vm.prank(USER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, SERVICE_FEE));
        bridge.createWithdrawal(SERVICE_FEE, SERVICE_FEE, hex"01", bytes32(0));
    }

    function testWithdrawalRejectsAmountsOutsideCanisterBounds() public {
        uint256 overflow = uint256(type(uint128).max) + 1;
        vm.startPrank(USER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.ValueExceedsU128.selector, overflow));
        bridge.createWithdrawal(overflow, SERVICE_FEE, hex"01", bytes32(0));
        vm.expectRevert(abi.encodeWithSelector(IBridge.ValueExceedsU128.selector, overflow));
        bridge.createWithdrawal(100, overflow, hex"01", bytes32(0));
        vm.stopPrank();
    }

    function testPrincipalValidationAndPauseRemainBurnGuards() public {
        bytes memory thirtyBytes = new bytes(30);
        vm.startPrank(USER);
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidPrincipal.selector, bytes("")));
        bridge.createWithdrawal(100, SERVICE_FEE, bytes(""), bytes32(0));
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidPrincipal.selector, thirtyBytes));
        bridge.createWithdrawal(100, SERVICE_FEE, thirtyBytes, bytes32(0));
        vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidPrincipal.selector, hex"04"));
        bridge.createWithdrawal(100, SERVICE_FEE, hex"04", bytes32(0));
        vm.stopPrank();

        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.pauseWithdrawals();
        vm.prank(USER);
        vm.expectRevert(IBridge.WithdrawalsArePaused.selector);
        bridge.createWithdrawal(100, SERVICE_FEE, hex"01", bytes32(0));
    }

    function testTransferFailureRollsBackCommitAndId() public {
        vm.prank(USER);
        (bool succeeded,) =
            address(bridge).call(abi.encodeCall(IBridge.createWithdrawal, (1_001, SERVICE_FEE, hex"01", bytes32(0))));
        assert(!succeeded);
        assert(bridge.nextWithdrawalId() == 1);
        assert(bridge.getWithdrawal(1).status == IBridge.WithdrawalStatus.None);
        assert(token.totalSupply() == 1_000);
    }

    function testProcessedDepositIdCannotBeReplayedAfterWithdrawal() public {
        uint256 first = _createWithdrawal(400, SERVICE_FEE, hex"01", bytes32(0));
        assert(first == 1);
        assert(bridge.getWithdrawal(first).status == IBridge.WithdrawalStatus.Committed);
        assert(token.totalSupply() == 600);

        IBridge.MintAuthorization memory replay = _authorization(keccak256("withdrawal-funding"), USER, 710);
        bytes memory replaySignature = _signMintAuthorization(BRIDGE_SIGNER_KEY, bridge, replay);
        vm.expectRevert(
            abi.encodeWithSelector(IBridge.DepositAlreadyProcessed.selector, keccak256("withdrawal-funding"))
        );
        bridge.mintDepositWithAuthorization(replay, replaySignature);
        assert(token.totalSupply() == 600);
    }

    function testMultipleWithdrawalsInOneTransactionRevertAtomically() public {
        WithdrawalBatcher batcher = new WithdrawalBatcher();
        _mint(keccak256("batcher-funding"), address(batcher), 710);

        vm.expectRevert(IBridge.MultipleWithdrawalsInTransaction.selector);
        batcher.createTwice(bridge, token);

        assert(token.balanceOf(address(batcher)) == 700);
        assert(token.totalSupply() == 1_700);
        assert(bridge.nextWithdrawalId() == 1);
        assert(bridge.getWithdrawal(1).status == IBridge.WithdrawalStatus.None);
    }

    function testFuzzCommittedQuote(uint256 amountSeed, uint256 feeSeed, bytes32 subaccount) public {
        uint256 fee = feeSeed % (MAX_SERVICE_FEE + 1);
        vm.prank(RUNTIME_ADMINISTRATOR);
        bridge.setServiceFee(fee);
        uint256 amount = fee + 1 + (amountSeed % (1_000 - fee));
        uint256 id = _createWithdrawal(amount, fee, hex"01", subaccount);
        IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(id);
        assert(withdrawal.amountOut + withdrawal.chargedServiceFee == withdrawal.amount);
        assert(withdrawal.chargedServiceFee <= withdrawal.maxServiceFee);
        assert(withdrawal.status == IBridge.WithdrawalStatus.Committed);
    }

    function _createWithdrawal(uint256 amount, uint256 maxServiceFee, bytes memory owner, bytes32 subaccount)
        private
        returns (uint256)
    {
        vm.prank(USER);
        token.approve(address(bridge), amount);
        vm.prank(USER);
        return bridge.createWithdrawal(amount, maxServiceFee, owner, subaccount);
    }

    function _mint(bytes32 depositId, address recipient, uint256 grossAmount) private {
        IBridge.MintAuthorization memory authorization = _authorization(depositId, recipient, grossAmount);
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, bridge, authorization, address(this));
    }

    function _authorization(bytes32 depositId, address recipient, uint256 grossAmount)
        private
        view
        returns (IBridge.MintAuthorization memory)
    {
        return IBridge.MintAuthorization({
            depositId: depositId,
            recipient: recipient,
            grossAmount: grossAmount,
            maxServiceFee: SERVICE_FEE,
            chargedServiceFee: SERVICE_FEE,
            deadline: block.timestamp + 30 minutes,
            authorizationEpoch: bridge.mintAuthorizationEpoch()
        });
    }
}
