// contracts/src/interfaces: define the Base Bridge ABI consumed by later contract and canister phases.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBSNS} from "./IBSNS.sol";

/// @notice Phase 1E frozen Bridge interface used by the concrete implementation and ABI checks.
interface IBridge {
    enum WithdrawalStatus {
        None,
        Pending,
        Releasing,
        Released,
        Refunded
    }

    struct DepositMintRequest {
        bytes32 depositId;
        address recipient;
        uint256 grossAmount;
        uint256 maxServiceFee;
        uint256 chargedServiceFee;
    }

    struct Withdrawal {
        address requester;
        uint256 amount;
        uint256 minAmountOut;
        bytes owner;
        bytes32 subaccount;
        WithdrawalStatus status;
        uint256 amountOut;
        uint256 serviceFee;
        uint256 ledgerFee;
        uint256 ledgerBlockIndex;
    }

    struct BridgeSnapshot {
        uint256 blockNumber;
        uint256 blockTimestamp;
        address bridgeSigner;
        uint256 serviceFee;
        uint256 maxServiceFee;
        uint256 perDepositLimit;
        uint256 mintWindowLimit;
        uint64 mintWindowDuration;
        uint64 mintWindowStartedAt;
        uint256 mintedInWindow;
        bool depositMintsPaused;
        bool withdrawalsPaused;
    }

    event DepositMinted(
        bytes32 indexed depositId,
        address indexed recipient,
        uint256 grossAmount,
        uint256 serviceFee,
        uint256 mintedAmount
    );
    event WithdrawalCreated(
        uint256 indexed withdrawalId,
        address indexed requester,
        uint256 amount,
        uint256 minAmountOut,
        bytes owner,
        bytes32 subaccount
    );
    event WithdrawalReleaseCancelled(uint256 indexed withdrawalId);
    event WithdrawalReleased(
        uint256 indexed withdrawalId, uint256 amountOut, uint256 serviceFee, uint256 ledgerFee, uint256 ledgerBlockIndex
    );
    event WithdrawalRefunded(uint256 indexed withdrawalId, address indexed requester, uint256 amount);
    event ServiceFeeChanged(address indexed caller, uint256 previousFee, uint256 newFee);
    event DepositMintsPaused(address indexed caller);
    event DepositMintsUnpaused(address indexed caller);
    event WithdrawalsPaused(address indexed caller);
    event WithdrawalsUnpaused(address indexed caller);
    event BridgeSignerChanged(address indexed previousSigner, address indexed newSigner);
    event RuntimeAdministratorChanged(address indexed previousAdministrator, address indexed newAdministrator);
    event BaseAdminTimelockChanged(address indexed previousTimelock, address indexed newTimelock);

    error ZeroAddress();
    error RoleAddressesMustDiffer();
    error InvalidAmount(uint256 amount);
    error InvalidPrincipal(bytes owner);
    error InvalidMinAmountOut(uint256 minAmountOut, uint256 amount);
    error InvalidServiceFee(uint256 serviceFee, uint256 maximumServiceFee);
    error ServiceFeeExceedsUserMaximum(uint256 serviceFee, uint256 userMaximum);
    error DepositAlreadyProcessed(bytes32 depositId);
    error DepositMintLimitExceeded(uint256 mintAmount, uint256 limit);
    error MintWindowLimitExceeded(uint256 requestedAmount, uint256 availableAmount);
    error DepositMintsArePaused();
    error WithdrawalsArePaused();
    error WithdrawalNotFound(uint256 withdrawalId);
    error TokenTransferFailed();
    error InvalidWithdrawalStatus(uint256 withdrawalId, WithdrawalStatus currentStatus);
    error SettlementAmountsMismatch(uint256 amount, uint256 amountOut, uint256 serviceFee, uint256 ledgerFee);
    error ReleaseAcknowledgementMismatch(uint256 withdrawalId);
    error LedgerBlockAlreadyAcknowledged(uint256 ledgerBlockIndex, uint256 existingWithdrawalId);
    error UnauthorizedBridgeSigner(address caller);
    error UnauthorizedRuntimeAdministrator(address caller);
    error UnauthorizedBaseAdmin(address caller);
    error TimelockCandidateHasNoCode(address candidate);
    error TimelockCandidateIntrospectionFailed(address candidate);
    error TimelockCandidateDelayTooShort(address candidate, uint256 suppliedDelay, uint256 minimumDelay);
    error TimelockCandidateMissingSelfAdmin(address candidate);

    function mintDeposit(DepositMintRequest calldata request) external;

    function createWithdrawal(uint256 amount, uint256 minAmountOut, bytes calldata owner, bytes32 subaccount)
        external
        returns (uint256 withdrawalId);

    function cancelRelease(uint256 withdrawalId) external;

    function acknowledgeRelease(
        uint256 withdrawalId,
        uint256 amountOut,
        uint256 serviceFee,
        uint256 ledgerFee,
        uint256 ledgerBlockIndex
    ) external;

    function refundWithdrawal(uint256 withdrawalId) external;

    function bridgeSnapshot() external view returns (BridgeSnapshot memory);

    function bsns() external view returns (IBSNS);

    function bridgeSigner() external view returns (address);

    function runtimeAdministrator() external view returns (address);

    function baseAdminTimelock() external view returns (address);

    function serviceFee() external view returns (uint256);

    function MAX_SERVICE_FEE() external view returns (uint256);

    function perDepositLimit() external view returns (uint256);

    function mintWindowLimit() external view returns (uint256);

    function mintWindowDuration() external view returns (uint64);

    function mintWindowStartedAt() external view returns (uint64);

    function mintedInWindow() external view returns (uint256);

    function depositMintsPaused() external view returns (bool);

    function withdrawalsPaused() external view returns (bool);

    function nextWithdrawalId() external view returns (uint256);

    function isDepositProcessed(bytes32 depositId) external view returns (bool);

    function getWithdrawal(uint256 withdrawalId) external view returns (Withdrawal memory);

    function pauseDepositMints() external;

    function pauseWithdrawals() external;

    function unpauseDepositMints() external;

    function unpauseWithdrawals() external;

    function setServiceFee(uint256 newServiceFee) external;

    function rotateBridgeSigner(address newSigner) external;

    function rotateRuntimeAdministrator(address newAdministrator) external;

    function rotateBaseAdminTimelock(address newTimelock) external;
}
