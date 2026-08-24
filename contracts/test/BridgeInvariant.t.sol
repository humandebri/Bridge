// contracts/test: stateful fuzz Bridge accounting, irreversible Withdrawal commitments, and admin safety predicates.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {StdInvariant} from "./StdInvariant.sol";
import {TestBase} from "./TestBase.sol";

contract BridgeInvariantHandler is TestBase {
    uint256 internal constant BRIDGE_SIGNER_KEY = 0xA11CE;
    address internal constant RUNTIME_ADMINISTRATOR = address(0x22);
    address internal BASE_ADMIN_TIMELOCK;

    Bridge private immutable _bridge;
    IBSNS private immutable _token;
    uint256 public cumulativeDepositMinted;
    uint256 public committedAmount;
    uint256 public depositNonce = 100;
    uint256 public withdrawalCount;
    uint256 public lastObservedEpoch = 1;
    uint256 private currentSignerKey = BRIDGE_SIGNER_KEY;
    bool public epochDecreased;

    constructor() {
        address bridgeSigner = vm.addr(BRIDGE_SIGNER_KEY);
        BASE_ADMIN_TIMELOCK = _deployTestTimelock(address(0x33));
        _bridge = new Bridge(
            bridgeSigner,
            RUNTIME_ADMINISTRATOR,
            BASE_ADMIN_TIMELOCK,
            _timelockCodeHash(BASE_ADMIN_TIMELOCK),
            1_000,
            100_000,
            1 hours,
            1,
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

    function submitAuthorization(uint256 seed) external {
        uint256 fee = _bridge.serviceFee();
        uint256 grossAmount = fee + 1 + (seed % 300);
        bytes32 depositId = keccak256(abi.encode("handler-deposit", depositNonce++));
        IBridge.MintAuthorization memory authorization = _authorization(depositId, address(this), grossAmount, fee);
        bytes memory signature = _signMintAuthorization(currentSignerKey, _bridge, authorization);
        (bool succeeded,) =
            address(_bridge).call(abi.encodeCall(IBridge.mintDepositWithAuthorization, (authorization, signature)));
        if (succeeded) {
            cumulativeDepositMinted += grossAmount - fee;
        }
    }

    function submitExpiredAuthorization(uint256 seed) external {
        uint256 fee = _bridge.serviceFee();
        IBridge.MintAuthorization memory authorization =
            _authorization(keccak256(abi.encode("expired", seed)), address(this), fee + 1, fee);
        authorization.deadline = 0;
        bytes memory signature = _signMintAuthorization(currentSignerKey, _bridge, authorization);
        (bool succeeded,) =
            address(_bridge).call(abi.encodeCall(IBridge.mintDepositWithAuthorization, (authorization, signature)));
        require(!succeeded, "expired authorization accepted");
    }

    function replayProcessedDeposit() external {
        uint256 fee = _bridge.serviceFee();
        IBridge.MintAuthorization memory authorization =
            _authorization(keccak256("invariant-0"), address(this), 1_010, fee);
        bytes memory signature = _signMintAuthorization(currentSignerKey, _bridge, authorization);
        (bool succeeded,) =
            address(_bridge).call(abi.encodeCall(IBridge.mintDepositWithAuthorization, (authorization, signature)));
        require(!succeeded, "processed deposit replayed");
    }

    function submitMutatedAuthorization(uint256 seed) external {
        uint256 fee = _bridge.serviceFee();
        IBridge.MintAuthorization memory authorization =
            _authorization(keccak256(abi.encode("mutated", seed)), address(this), fee + 1, fee);
        bytes memory signature = _signMintAuthorization(currentSignerKey, _bridge, authorization);
        authorization.recipient = address(0xBEEF);
        (bool succeeded,) =
            address(_bridge).call(abi.encodeCall(IBridge.mintDepositWithAuthorization, (authorization, signature)));
        require(!succeeded, "mutated authorization accepted");
    }

    function pauseAndUnpauseDeposits() external {
        vm.prank(RUNTIME_ADMINISTRATOR);
        _bridge.pauseDepositMints();
        uint256 currentEpoch = _bridge.mintAuthorizationEpoch();
        if (currentEpoch < lastObservedEpoch) {
            epochDecreased = true;
        }
        lastObservedEpoch = currentEpoch;
        uint256 nextKey = currentSignerKey + 1;
        vm.prank(BASE_ADMIN_TIMELOCK);
        (bool rotated,) = address(_bridge).call(abi.encodeCall(IBridge.rotateBridgeSigner, (vm.addr(nextKey))));
        if (!rotated) {
            return;
        }
        currentSignerKey = nextKey;
        vm.prank(BASE_ADMIN_TIMELOCK);
        _bridge.unpauseDepositMints();
    }

    function rotateSigner(uint256 seed) external {
        IBridge.MintAuthorization memory oldAuthorization =
            _authorization(keccak256(abi.encode("old-epoch", seed)), address(this), 11, 10);
        bytes memory oldSignature = _signMintAuthorization(currentSignerKey, _bridge, oldAuthorization);
        uint256 nextKey = seed % (type(uint96).max - 1) + 1;
        address nextSigner = vm.addr(nextKey);
        if (
            nextSigner == address(0) || nextSigner == RUNTIME_ADMINISTRATOR || nextSigner == BASE_ADMIN_TIMELOCK
                || nextSigner == _bridge.bridgeSigner()
        ) {
            return;
        }
        vm.prank(BASE_ADMIN_TIMELOCK);
        (bool rotated,) = address(_bridge).call(abi.encodeCall(IBridge.rotateBridgeSigner, (nextSigner)));
        if (!rotated) {
            return;
        }
        currentSignerKey = nextKey;
        uint256 currentEpoch = _bridge.mintAuthorizationEpoch();
        if (currentEpoch <= lastObservedEpoch) {
            epochDecreased = true;
        }
        lastObservedEpoch = currentEpoch;
        (bool succeeded,) = address(_bridge)
            .call(abi.encodeCall(IBridge.mintDepositWithAuthorization, (oldAuthorization, oldSignature)));
        require(!succeeded, "old epoch authorization accepted");
    }

    function createWithdrawal(uint256 seed) external {
        address user = address(this);
        uint256 balance = _token.balanceOf(user);
        if (balance == 0) {
            return;
        }
        uint256 fee = _bridge.serviceFee();
        if (balance <= fee) {
            return;
        }
        uint256 amount = fee + 1 + (seed % (balance - fee));
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
            address(_bridge).call(abi.encodeCall(IBridge.createWithdrawal, (amount, fee, owner, subaccount)));
        if (succeeded) {
            withdrawalCount++;
            committedAmount += amount;
        }
    }

    function _initialMint(bytes32 depositId, address recipient) private {
        IBridge.MintAuthorization memory authorization = _authorization(depositId, recipient, 1_010, 10);
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, _bridge, authorization, address(this));
    }

    function _authorization(bytes32 depositId, address recipient, uint256 grossAmount, uint256 fee)
        private
        view
        returns (IBridge.MintAuthorization memory)
    {
        return IBridge.MintAuthorization({
            depositId: depositId,
            recipient: recipient,
            grossAmount: grossAmount,
            maxServiceFee: fee,
            chargedServiceFee: fee,
            deadline: block.timestamp + 15 minutes,
            authorizationEpoch: _bridge.mintAuthorizationEpoch()
        });
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
        assert(token.totalSupply() + handler.committedAmount() == handler.cumulativeDepositMinted());
    }

    function invariantTrackedBalancesEqualSupply() public view {
        assert(token.balanceOf(address(handler)) == token.totalSupply());
    }

    function invariantWithdrawalRecordsRemainTerminalAndValid() public view {
        for (uint256 withdrawalId = 1; withdrawalId <= handler.withdrawalCount(); ++withdrawalId) {
            IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
            assert(withdrawal.status == IBridge.WithdrawalStatus.Committed);
            assert(withdrawal.amountOut + withdrawal.chargedServiceFee == withdrawal.amount);
            assert(withdrawal.chargedServiceFee <= withdrawal.maxServiceFee);
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
        assert(!handler.epochDecreased());
        assert(bridge.mintAuthorizationEpoch() >= handler.lastObservedEpoch());
    }
}
