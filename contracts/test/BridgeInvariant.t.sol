// contracts/test: stateful fuzz the Bridge accounting, terminal Withdrawal states, and admin safety predicates.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {StdInvariant} from "./StdInvariant.sol";
import {TestBase} from "./TestBase.sol";

contract BridgeInvariantHandler is TestBase {
    address internal constant RUNTIME_ADMINISTRATOR = address(0x22);
    address internal BASE_ADMIN_TIMELOCK;

    Bridge private immutable _bridge;
    IBSNS private immutable _token;
    uint256 public cumulativeDepositMinted;
    uint256 public pendingAmount;
    uint256 public releasedAmount;
    uint256 public depositNonce = 100;
    uint256 public withdrawalCount;
    uint256 public ledgerBlockNonce;

    constructor() {
        BASE_ADMIN_TIMELOCK = _deployTestTimelock(address(0x33));
        _bridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            address(this),
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            1_000,
            100_000,
            1 hours,
            100,
            10
        );
        _token = _bridge.bsns();
        vm.startPrank(BASE_ADMIN_TIMELOCK);
        _bridge.unpauseDepositMints();
        _bridge.unpauseWithdrawals();
        vm.stopPrank();
        _initialMint(keccak256("invariant-0"), address(this));
        _initialMint(keccak256("invariant-1"), address(this));
        _initialMint(keccak256("invariant-2"), address(this));
        _initialMint(keccak256("invariant-3"), address(this));
        cumulativeDepositMinted = 4_000;
    }

    function bridgeAddress() external view returns (address) {
        return address(_bridge);
    }

    function tokenAddress() external view returns (address) {
        return address(_token);
    }

    function mintDeposit(uint256 seed) external {
        uint256 fee = _bridge.serviceFee();
        uint256 grossAmount = fee + 1 + (seed % 300);
        bytes32 depositId = keccak256(abi.encode("handler-deposit", depositNonce++));
        IBridge.DepositMintRequest memory request =
            IBridge.DepositMintRequest(depositId, address(this), grossAmount, fee, fee);
        (bool succeeded,) = address(_bridge).call(abi.encodeCall(IBridge.mintDeposit, (request)));
        if (succeeded) {
            cumulativeDepositMinted += grossAmount - fee;
        }
    }

    function createWithdrawal(uint256 seed) external {
        address user = address(this);
        uint256 balance = _token.balanceOf(user);
        if (balance == 0) {
            return;
        }
        uint256 amount = 1 + (seed % balance);
        uint256 minAmountOut = 1 + (seed % amount);
        uint256 principalTag = seed % 200 + 1;
        if (principalTag == 4) {
            principalTag = 5;
        }
        bytes memory owner = new bytes(1);
        assembly {
            mstore8(add(owner, 0x20), principalTag)
        }
        bytes32 subaccount = bytes32(seed);
        _token.approve(address(_bridge), amount);
        (bool succeeded,) =
            address(_bridge).call(abi.encodeCall(IBridge.createWithdrawal, (amount, minAmountOut, owner, subaccount)));
        if (succeeded) {
            withdrawalCount++;
            pendingAmount += amount;
        }
    }

    function acknowledgeRelease(uint256 seed) external {
        if (withdrawalCount == 0) {
            return;
        }
        uint256 withdrawalId = 1 + (seed % withdrawalCount);
        IBridge.Withdrawal memory withdrawal = _bridge.getWithdrawal(withdrawalId);
        if (withdrawal.status == IBridge.WithdrawalStatus.Releasing) {
            uint256 amountOut = withdrawal.minAmountOut + (seed % (withdrawal.amount - withdrawal.minAmountOut + 1));
            uint256 remaining = withdrawal.amount - amountOut;
            uint256 feeLimit = remaining < _bridge.MAX_SERVICE_FEE() ? remaining : _bridge.MAX_SERVICE_FEE();
            uint256 serviceFee = feeLimit == 0 ? 0 : seed % (feeLimit + 1);
            uint256 ledgerFee = remaining - serviceFee;
            uint256 ledgerBlockIndex = ledgerBlockNonce++;
            (bool succeeded,) = address(_bridge)
                .call(
                    abi.encodeCall(
                        IBridge.acknowledgeRelease, (withdrawalId, amountOut, serviceFee, ledgerFee, ledgerBlockIndex)
                    )
                );
            if (succeeded) {
                pendingAmount -= withdrawal.amount;
                releasedAmount += withdrawal.amount;
            }
        } else if (withdrawal.status == IBridge.WithdrawalStatus.Released && seed % 2 == 0) {
            _bridge.acknowledgeRelease(
                withdrawalId,
                withdrawal.amountOut,
                withdrawal.serviceFee,
                withdrawal.ledgerFee,
                withdrawal.ledgerBlockIndex
            );
        }
    }

    function cancelRelease(uint256 seed) external {
        if (withdrawalCount == 0) {
            return;
        }
        uint256 withdrawalId = 1 + (seed % withdrawalCount);
        if (_bridge.getWithdrawal(withdrawalId).status == IBridge.WithdrawalStatus.Releasing) {
            _bridge.cancelRelease(withdrawalId);
        }
    }

    function refundWithdrawal(uint256 seed) external {
        if (withdrawalCount == 0) {
            return;
        }
        uint256 withdrawalId = 1 + (seed % withdrawalCount);
        IBridge.Withdrawal memory withdrawal = _bridge.getWithdrawal(withdrawalId);
        if (withdrawal.status == IBridge.WithdrawalStatus.Releasing) {
            _bridge.cancelRelease(withdrawalId);
            withdrawal = _bridge.getWithdrawal(withdrawalId);
        }
        if (withdrawal.status != IBridge.WithdrawalStatus.Pending) {
            return;
        }
        (bool succeeded,) = address(_bridge).call(abi.encodeCall(IBridge.refundWithdrawal, (withdrawalId)));
        if (succeeded) {
            pendingAmount -= withdrawal.amount;
        }
    }

    function _initialMint(bytes32 depositId, address recipient) private {
        _bridge.mintDeposit(IBridge.DepositMintRequest(depositId, recipient, 1_010, 10, 10));
    }
}

contract BridgeInvariantTest is TestBase, StdInvariant {
    BridgeInvariantHandler private handler;
    Bridge private bridge;
    IBSNS private token;

    function setUp() public {
        handler = new BridgeInvariantHandler();
        bridge = Bridge(handler.bridgeAddress());
        token = IBSNS(handler.tokenAddress());
        targetContract(address(handler));
    }

    function invariantExposureIsConserved() public view {
        assert(
            token.totalSupply() + handler.pendingAmount() + handler.releasedAmount()
                == handler.cumulativeDepositMinted()
        );
    }

    function invariantTrackedBalancesEqualSupply() public view {
        assert(token.balanceOf(address(handler)) == token.totalSupply());
    }

    function invariantWithdrawalRecordsRemainTerminalAndValid() public view {
        for (uint256 withdrawalId = 1; withdrawalId <= handler.withdrawalCount(); ++withdrawalId) {
            IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
            if (withdrawal.status == IBridge.WithdrawalStatus.Released) {
                assert(withdrawal.amountOut + withdrawal.serviceFee + withdrawal.ledgerFee == withdrawal.amount);
                assert(withdrawal.amountOut >= withdrawal.minAmountOut);
                assert(withdrawal.serviceFee <= bridge.MAX_SERVICE_FEE());
            } else if (withdrawal.status == IBridge.WithdrawalStatus.Refunded) {
                assert(withdrawal.amountOut == 0);
                assert(withdrawal.serviceFee == 0);
                assert(withdrawal.ledgerFee == 0);
                assert(withdrawal.ledgerBlockIndex == 0);
            } else {
                assert(
                    withdrawal.status == IBridge.WithdrawalStatus.Pending
                        || withdrawal.status == IBridge.WithdrawalStatus.Releasing
                );
            }
        }
    }

    function invariantRolesAndFeeRemainSafe() public view {
        address signer = bridge.bridgeSigner();
        address administrator = bridge.runtimeAdministrator();
        address timelock = bridge.baseAdminTimelock();
        assert(signer != address(0) && administrator != address(0) && timelock != address(0));
        assert(signer != administrator && signer != timelock && administrator != timelock);
        assert(bridge.serviceFee() <= bridge.MAX_SERVICE_FEE());
        assert(bridge.nextWithdrawalId() == handler.withdrawalCount() + 1);
    }
}
