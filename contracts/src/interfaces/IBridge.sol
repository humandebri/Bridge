// contracts/src/interfaces: define the Base Bridge ABI consumed by later contract and canister phases.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBSNS} from "./IBSNS.sol";

/// @notice Phase 1E frozen Bridge interface used by the concrete implementation and ABI checks.
interface IBridge {
    enum WithdrawalStatus {
        None,
        Committed
    }

    struct MintAuthorization {
        bytes32 depositId;
        address recipient;
        uint256 grossAmount;
        uint256 maxServiceFee;
        uint256 chargedServiceFee;
        uint256 deadline;
        uint256 authorizationEpoch;
    }

    struct Withdrawal {
        address requester;
        uint256 amount;
        uint256 maxServiceFee;
        uint256 chargedServiceFee;
        uint256 amountOut;
        bytes owner;
        bytes32 subaccount;
        WithdrawalStatus status;
    }

    struct BridgeSnapshot {
        uint256 blockNumber;
        uint256 blockTimestamp;
        address bridgeSigner;
        uint256 mintAuthorizationEpoch;
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
        bytes32 indexed authorizationDigest,
        uint256 grossAmount,
        uint256 serviceFee,
        uint256 mintedAmount
    );
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
    event ServiceFeeChanged(address indexed caller, uint256 previousFee, uint256 newFee);
    event DepositMintsPaused(address indexed caller);
    event DepositMintsUnpaused(address indexed caller);
    event WithdrawalsPaused(address indexed caller);
    event WithdrawalsUnpaused(address indexed caller);
    event BridgeSignerChanged(address indexed previousSigner, address indexed newSigner);
    event MintAuthorizationEpochChanged(address indexed caller, uint256 previousEpoch, uint256 newEpoch);
    event RuntimeAdministratorChanged(address indexed previousAdministrator, address indexed newAdministrator);
    event BaseAdminTimelockChanged(address indexed previousTimelock, address indexed newTimelock);

    error ZeroAddress();
    error RoleAddressesMustDiffer();
    error InvalidAmount(uint256 amount);
    error InvalidPrincipal(bytes owner);
    error InvalidServiceFee(uint256 serviceFee, uint256 maximumServiceFee);
    error ValueExceedsU128(uint256 value);
    error BlockTimestampExceedsU64(uint256 timestamp);
    error InvalidMintWindowDuration(uint64 suppliedDuration, uint64 minimumDuration, uint64 maximumDuration);
    error ServiceFeeExceedsUserMaximum(uint256 serviceFee, uint256 userMaximum);
    error DepositAlreadyProcessed(bytes32 depositId);
    error DepositMintLimitExceeded(uint256 mintAmount, uint256 limit);
    error MintWindowLimitExceeded(uint256 requestedAmount, uint256 availableAmount);
    error DepositMintsArePaused();
    error MintAuthorizationExpired(uint256 currentTimestamp, uint256 deadline);
    error MintAuthorizationEpochMismatch(uint256 suppliedEpoch, uint256 currentEpoch);
    error InvalidMintAuthorizationSignature();
    error WithdrawalsArePaused();
    error MultipleWithdrawalsInTransaction();
    error InvalidMintRecipient(address recipient);
    error TokenTransferFailed();
    error UnauthorizedRuntimeAdministrator(address caller);
    error UnauthorizedBaseAdmin(address caller);
    error TimelockCandidateHasNoCode(address candidate);
    error TimelockCandidateCodeHashMismatch(address candidate, bytes32 actualCodeHash, bytes32 expectedCodeHash);
    error TimelockCandidateIntrospectionFailed(address candidate);
    error TimelockCandidateDelayTooShort(address candidate, uint256 suppliedDelay, uint256 minimumDelay);
    error TimelockCandidateDelayTooLong(address candidate, uint256 suppliedDelay, uint256 maximumDelay);
    error TimelockCandidateMissingSelfAdmin(address candidate);
    error TimelockCandidateInvalidRoleMember(address candidate, bytes32 role, address member);
    error TimelockCandidateHasPendingOperations(address candidate, uint256 pendingOperationCount);

    function mintDepositWithAuthorization(MintAuthorization calldata authorization, bytes calldata signature) external;

    function createWithdrawal(uint256 amount, uint256 maxServiceFee, bytes calldata owner, bytes32 subaccount)
        external
        returns (uint256 withdrawalId);

    function bridgeSnapshot() external view returns (BridgeSnapshot memory);

    function bsns() external view returns (IBSNS);

    function bridgeSigner() external view returns (address);

    function mintAuthorizationEpoch() external view returns (uint256);

    function runtimeAdministrator() external view returns (address);

    function baseAdminTimelock() external view returns (address);

    function approvedTimelockRuntimeCodeHash() external view returns (bytes32);

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
