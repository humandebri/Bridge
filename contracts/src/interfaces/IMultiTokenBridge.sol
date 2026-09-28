// contracts/src/interfaces: define the shared Base Bridge ABI for additional IC assets.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

interface IMultiTokenBridge {
    enum WithdrawalStatus {
        None,
        Committed
    }

    struct AssetRegistration {
        bytes32 assetId;
        string tokenName;
        string tokenSymbol;
        uint8 tokenDecimals;
        uint256 perDepositLimit;
        uint256 mintWindowLimit;
        uint64 mintWindowDuration;
        uint256 minServiceFee;
        uint256 maxServiceFee;
        uint256 initialServiceFee;
    }

    struct AssetSnapshot {
        address token;
        uint256 assetEpoch;
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

    struct MintAuthorization {
        bytes32 assetId;
        bytes32 depositId;
        address recipient;
        uint256 grossAmount;
        uint256 maxServiceFee;
        uint256 chargedServiceFee;
        uint256 deadline;
        uint256 globalEpoch;
        uint256 assetEpoch;
    }

    struct Withdrawal {
        bytes32 assetId;
        address requester;
        uint256 amount;
        uint256 maxServiceFee;
        uint256 chargedServiceFee;
        uint256 amountOut;
        bytes owner;
        bytes32 subaccount;
        WithdrawalStatus status;
    }

    event AssetRegistered(
        bytes32 indexed assetId,
        address indexed token,
        string tokenName,
        string tokenSymbol,
        uint8 tokenDecimals,
        uint256 perDepositLimit,
        uint256 mintWindowLimit,
        uint64 mintWindowDuration,
        uint256 minServiceFee,
        uint256 maxServiceFee,
        uint256 initialServiceFee
    );
    event DepositMinted(
        bytes32 indexed assetId,
        bytes32 indexed depositId,
        address indexed recipient,
        bytes32 authorizationDigest,
        uint256 grossAmount,
        uint256 serviceFee,
        uint256 mintedAmount
    );
    event WithdrawalCommitted(
        uint256 indexed withdrawalId,
        bytes32 indexed assetId,
        address indexed requester,
        uint256 amount,
        uint256 maxServiceFee,
        uint256 chargedServiceFee,
        uint256 amountOut,
        bytes owner,
        bytes32 subaccount
    );
    event AssetServiceFeeChanged(bytes32 indexed assetId, address indexed caller, uint256 previousFee, uint256 newFee);
    event AssetDepositMintsPaused(bytes32 indexed assetId, address indexed caller);
    event AssetDepositMintsUnpaused(bytes32 indexed assetId, address indexed caller);
    event AssetWithdrawalsPaused(bytes32 indexed assetId, address indexed caller);
    event AssetWithdrawalsUnpaused(bytes32 indexed assetId, address indexed caller);
    event BridgeSignerChanged(address indexed previousSigner, address indexed newSigner);
    event GlobalEpochChanged(address indexed caller, uint256 previousEpoch, uint256 newEpoch);
    event AssetEpochChanged(bytes32 indexed assetId, address indexed caller, uint256 previousEpoch, uint256 newEpoch);
    event GlobalDepositMintsPaused(address indexed caller);
    event GlobalDepositMintsUnpaused(address indexed caller);
    event GlobalWithdrawalsPaused(address indexed caller);
    event GlobalWithdrawalsUnpaused(address indexed caller);
    event RuntimeAdministratorChanged(address indexed previousAdministrator, address indexed newAdministrator);

    error ZeroAddress();
    error ZeroAssetId();
    error AssetAlreadyRegistered(bytes32 assetId);
    error TokenAlreadyRegistered(address token);
    error UnknownAsset(bytes32 assetId);
    error EmptyTokenMetadata();
    error TokenMetadataTooLong(uint256 nameLength, uint256 symbolLength);
    error AssetLimitReached(uint256 maximumAssets);
    error RoleAddressesMustDiffer();
    error InvalidAmount(uint256 amount);
    error InvalidPrincipal(bytes owner);
    error InvalidServiceFee(uint256 serviceFee, uint256 maximumServiceFee);
    error ValueExceedsU128(uint256 value);
    error BlockTimestampExceedsU64(uint256 timestamp);
    error InvalidMintWindowDuration(uint64 suppliedDuration, uint64 minimumDuration, uint64 maximumDuration);
    error ServiceFeeExceedsUserMaximum(uint256 serviceFee, uint256 userMaximum);
    error BridgeSignerRotationRequired();
    error BridgeSignerAlreadyRetired(address signer);
    error DepositAlreadyProcessed(bytes32 assetId, bytes32 depositId);
    error DepositMintLimitExceeded(uint256 mintAmount, uint256 limit);
    error MintWindowLimitExceeded(uint256 requestedAmount, uint256 availableAmount);
    error DepositMintsArePaused(bytes32 assetId);
    error MintAuthorizationExpired(uint256 currentTimestamp, uint256 deadline);
    error MintAuthorizationDeadlineTooFar(uint256 deadline, uint256 maximumDeadline);
    error GlobalEpochMismatch(uint256 suppliedEpoch, uint256 currentEpoch);
    error AssetEpochMismatch(uint256 suppliedEpoch, uint256 currentEpoch);
    error InvalidMintAuthorizationSignature();
    error WithdrawalsArePaused(bytes32 assetId);
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

    function registerAsset(AssetRegistration calldata registration) external returns (address token);
    function mintDepositWithAuthorization(MintAuthorization calldata authorization, bytes calldata signature) external;
    function createWithdrawal(
        bytes32 assetId,
        uint256 amount,
        uint256 maxServiceFee,
        bytes calldata owner,
        bytes32 subaccount
    ) external returns (uint256 withdrawalId);
    function assetSnapshot(bytes32 assetId) external view returns (AssetSnapshot memory);
    function tokenForAsset(bytes32 assetId) external view returns (address);
    function isDepositProcessed(bytes32 assetId, bytes32 depositId) external view returns (bool);
    function getWithdrawal(uint256 withdrawalId) external view returns (Withdrawal memory);
    function pauseAssetDepositMints(bytes32 assetId) external;
    function pauseAssetWithdrawals(bytes32 assetId) external;
    function pauseAllDepositMints() external;
    function pauseAllWithdrawals() external;
    function unpauseAssetDepositMints(bytes32 assetId) external;
    function unpauseAssetWithdrawals(bytes32 assetId) external;
    function unpauseAllDepositMints() external;
    function unpauseAllWithdrawals() external;
    function setAssetServiceFee(bytes32 assetId, uint256 newServiceFee) external;
    function rotateBridgeSigner(address newSigner) external;
    function rotateRuntimeAdministrator(address newAdministrator) external;
}
