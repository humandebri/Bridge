// contracts/src: implement one Base Bridge for multiple additional IC assets.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BSNS} from "./BSNS.sol";
import {IBSNS} from "./interfaces/IBSNS.sol";
import {IMultiTokenBridge} from "./interfaces/IMultiTokenBridge.sol";
import {BridgeAdministration} from "./libraries/BridgeAdministration.sol";
import {MintAuthorizationPolicy} from "./libraries/MintAuthorizationPolicy.sol";
import {DeploymentPolicy} from "bridge-deployment-policy/DeploymentPolicy.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";

interface IMultiTokenTimelockCandidate {
    function getMinDelay() external view returns (uint256);
    function hasRole(bytes32 role, address account) external view returns (bool);
    function roleMember(bytes32 role) external view returns (address);
    function pendingOperationCount() external view returns (uint256);
}

contract MultiTokenBridge is IMultiTokenBridge, EIP712 {
    struct AssetState {
        IBSNS token;
        uint128 minServiceFee;
        uint128 maxServiceFee;
        uint128 serviceFee;
        uint128 perDepositLimit;
        uint128 mintWindowLimit;
        uint128 mintedInWindow;
        uint64 mintWindowDuration;
        uint64 mintWindowStartedAt;
        uint256 assetEpoch;
        bool exists;
        bool depositMintsPaused;
        bool withdrawalsPaused;
    }

    struct StoredWithdrawal {
        bytes32 assetId;
        address requester;
        bool exists;
        uint128 amount;
        uint128 maxServiceFee;
        uint128 chargedServiceFee;
        bytes owner;
        bytes32 subaccount;
    }

    uint64 private constant MINIMUM_MINT_WINDOW_DURATION = 1 hours;
    uint64 private constant MAXIMUM_MINT_WINDOW_DURATION = 30 days;
    uint256 private constant MAXIMUM_ASSETS = 100;
    uint256 private constant MAXIMUM_TOKEN_NAME_BYTES = 64;
    uint256 private constant MAXIMUM_TOKEN_SYMBOL_BYTES = 16;
    uint256 private constant MAXIMUM_TIMELOCK_DELAY = 30 days;
    bytes32 private constant PROPOSER_ROLE = keccak256("PROPOSER_ROLE");
    bytes32 private constant CANCELLER_ROLE = keccak256("CANCELLER_ROLE");
    bytes32 private constant EXECUTOR_ROLE = keccak256("EXECUTOR_ROLE");
    bytes32 private constant WITHDRAWAL_TRANSACTION_SLOT = keccak256("kinic.multi-token-bridge.withdrawal.transaction");
    bytes32 private constant MINT_AUTHORIZATION_TYPEHASH = keccak256(
        "MintAuthorization(bytes32 assetId,bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 globalEpoch,uint256 assetEpoch)"
    );

    bytes32 public immutable approvedTimelockRuntimeCodeHash;
    address public immutable baseAdminTimelock;
    address public bridgeSigner;
    address public runtimeAdministrator;
    uint256 public globalEpoch = 1;
    uint256 public nextWithdrawalId = 1;
    uint256 public assetCount;
    bool public globalDepositMintsPaused = true;
    bool public globalWithdrawalsPaused = true;

    mapping(bytes32 assetId => AssetState state) private _assets;
    mapping(address token => bool registered) private _registeredTokens;
    mapping(bytes32 assetId => mapping(bytes32 depositId => bool processed)) private _processedDeposits;
    mapping(uint256 withdrawalId => StoredWithdrawal withdrawal) private _withdrawals;
    mapping(address signer => bool retired) private _retiredBridgeSigners;
    address private _bridgeSignerAtLastGlobalPause;

    modifier onlyRuntimeAdministrator() {
        if (msg.sender != runtimeAdministrator) {
            revert UnauthorizedRuntimeAdministrator(msg.sender);
        }
        _;
    }

    modifier onlyBaseAdminTimelock() {
        if (msg.sender != baseAdminTimelock) {
            revert UnauthorizedBaseAdmin(msg.sender);
        }
        _;
    }

    constructor(
        address initialBridgeSigner,
        address initialRuntimeAdministrator,
        address initialBaseAdminTimelock,
        bytes32 initialApprovedTimelockRuntimeCodeHash
    ) EIP712("IC Base Multi-Token Bridge", "1") {
        if (!BridgeAdministration.rolesAreNonzero(
                initialBridgeSigner, initialRuntimeAdministrator, initialBaseAdminTimelock
            )) {
            revert ZeroAddress();
        }
        if (!BridgeAdministration.rolesAreDistinct(
                initialBridgeSigner, initialRuntimeAdministrator, initialBaseAdminTimelock
            )) {
            revert RoleAddressesMustDiffer();
        }
        approvedTimelockRuntimeCodeHash = initialApprovedTimelockRuntimeCodeHash;
        _validateTimelockCandidate(initialBaseAdminTimelock);
        bridgeSigner = initialBridgeSigner;
        runtimeAdministrator = initialRuntimeAdministrator;
        baseAdminTimelock = initialBaseAdminTimelock;
    }

    function registerAsset(AssetRegistration calldata registration)
        external
        override
        onlyBaseAdminTimelock
        returns (address tokenAddress)
    {
        if (registration.assetId == bytes32(0)) revert ZeroAssetId();
        if (_assets[registration.assetId].exists) revert AssetAlreadyRegistered(registration.assetId);
        if (assetCount >= MAXIMUM_ASSETS) revert AssetLimitReached(MAXIMUM_ASSETS);
        if (bytes(registration.tokenName).length == 0 || bytes(registration.tokenSymbol).length == 0) {
            revert EmptyTokenMetadata();
        }
        if (
            bytes(registration.tokenName).length > MAXIMUM_TOKEN_NAME_BYTES
                || bytes(registration.tokenSymbol).length > MAXIMUM_TOKEN_SYMBOL_BYTES
        ) {
            revert TokenMetadataTooLong(bytes(registration.tokenName).length, bytes(registration.tokenSymbol).length);
        }
        if (
            registration.perDepositLimit == 0 || registration.mintWindowLimit == 0
                || registration.mintWindowDuration < MINIMUM_MINT_WINDOW_DURATION
                || registration.mintWindowDuration > MAXIMUM_MINT_WINDOW_DURATION || registration.minServiceFee == 0
                || registration.maxServiceFee < registration.minServiceFee
        ) {
            if (
                registration.mintWindowDuration < MINIMUM_MINT_WINDOW_DURATION
                    || registration.mintWindowDuration > MAXIMUM_MINT_WINDOW_DURATION
            ) {
                revert InvalidMintWindowDuration(
                    registration.mintWindowDuration, MINIMUM_MINT_WINDOW_DURATION, MAXIMUM_MINT_WINDOW_DURATION
                );
            }
            revert InvalidAmount(0);
        }
        // Consensus timestamp bounds the stored fixed-window anchor.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp > type(uint64).max) revert BlockTimestampExceedsU64(block.timestamp);
        _requireU128(registration.perDepositLimit);
        _requireU128(registration.mintWindowLimit);
        _requireU128(registration.minServiceFee);
        _requireU128(registration.maxServiceFee);
        _requireU128(registration.initialServiceFee);
        if (!BridgeAdministration.serviceFeeIsValid(
                registration.initialServiceFee, registration.minServiceFee, registration.maxServiceFee
            )) {
            revert InvalidServiceFee(registration.initialServiceFee, registration.maxServiceFee);
        }

        IBSNS token =
            new BSNS(registration.tokenName, registration.tokenSymbol, registration.tokenDecimals, address(this));
        tokenAddress = address(token);
        if (_registeredTokens[tokenAddress]) revert TokenAlreadyRegistered(tokenAddress);
        _registeredTokens[tokenAddress] = true;
        assetCount += 1;
        _assets[registration.assetId] = AssetState({
            token: token,
            minServiceFee: uint128(registration.minServiceFee),
            maxServiceFee: uint128(registration.maxServiceFee),
            serviceFee: uint128(registration.initialServiceFee),
            perDepositLimit: uint128(registration.perDepositLimit),
            mintWindowLimit: uint128(registration.mintWindowLimit),
            mintedInWindow: 0,
            mintWindowDuration: registration.mintWindowDuration,
            mintWindowStartedAt: uint64(block.timestamp),
            assetEpoch: 1,
            exists: true,
            depositMintsPaused: true,
            withdrawalsPaused: true
        });
        emit AssetRegistered(
            registration.assetId,
            tokenAddress,
            registration.tokenName,
            registration.tokenSymbol,
            registration.tokenDecimals,
            registration.perDepositLimit,
            registration.mintWindowLimit,
            registration.mintWindowDuration,
            registration.minServiceFee,
            registration.maxServiceFee,
            registration.initialServiceFee
        );
    }

    function mintDepositWithAuthorization(MintAuthorization calldata authorization, bytes calldata signature)
        external
        override
    {
        AssetState storage asset = _asset(authorization.assetId);
        if (globalDepositMintsPaused || asset.depositMintsPaused) {
            revert DepositMintsArePaused(authorization.assetId);
        }
        if (authorization.globalEpoch != globalEpoch) {
            revert GlobalEpochMismatch(authorization.globalEpoch, globalEpoch);
        }
        if (authorization.assetEpoch != asset.assetEpoch) {
            revert AssetEpochMismatch(authorization.assetEpoch, asset.assetEpoch);
        }
        (
            MintAuthorizationPolicy.RejectReason reason,
            MintAuthorizationPolicy.MintEffects memory effects,
            uint256 available
        ) = MintAuthorizationPolicy.evaluateMint(
            MintAuthorizationPolicy.MintTransitionInput({
                timestamp: block.timestamp,
                deadline: authorization.deadline,
                authorizationEpoch: authorization.globalEpoch,
                currentEpoch: globalEpoch,
                recipient: authorization.recipient,
                bridge: address(this),
                token: address(asset.token),
                grossAmount: authorization.grossAmount,
                maximumFee: authorization.maxServiceFee,
                chargedFee: authorization.chargedServiceFee,
                protocolMaximumFee: asset.maxServiceFee,
                perDepositLimit: asset.perDepositLimit,
                consumedInWindow: asset.mintedInWindow,
                windowLimit: asset.mintWindowLimit,
                windowStartedAt: asset.mintWindowStartedAt,
                windowDuration: asset.mintWindowDuration,
                paused: false,
                processed: _processedDeposits[authorization.assetId][authorization.depositId]
            })
        );
        _revertRejectedMint(reason, authorization, available, asset.maxServiceFee);

        bytes32 digest = _hashTypedDataV4(
            keccak256(
                abi.encode(
                    MINT_AUTHORIZATION_TYPEHASH,
                    authorization.assetId,
                    authorization.depositId,
                    authorization.recipient,
                    authorization.grossAmount,
                    authorization.maxServiceFee,
                    authorization.chargedServiceFee,
                    authorization.deadline,
                    authorization.globalEpoch,
                    authorization.assetEpoch
                )
            )
        );
        (address recovered, ECDSA.RecoverError error,) = ECDSA.tryRecoverCalldata(digest, signature);
        if (error != ECDSA.RecoverError.NoError || recovered != bridgeSigner) {
            revert InvalidMintAuthorizationSignature();
        }

        _processedDeposits[authorization.assetId][authorization.depositId] = true;
        asset.mintWindowStartedAt = effects.windowStartedAtAfter;
        asset.mintedInWindow = uint128(effects.windowConsumedAfter);
        asset.token.bridgeMint(authorization.recipient, effects.mintAmount);
        emit DepositMinted(
            authorization.assetId,
            authorization.depositId,
            authorization.recipient,
            digest,
            authorization.grossAmount,
            authorization.chargedServiceFee,
            effects.mintAmount
        );
    }

    function createWithdrawal(
        bytes32 assetId,
        uint256 amount,
        uint256 maxServiceFee,
        bytes calldata owner,
        bytes32 subaccount
    ) external override returns (uint256 withdrawalId) {
        AssetState storage asset = _asset(assetId);
        if (globalWithdrawalsPaused || asset.withdrawalsPaused) revert WithdrawalsArePaused(assetId);
        if (amount > type(uint128).max) revert ValueExceedsU128(amount);
        if (maxServiceFee > type(uint128).max) revert ValueExceedsU128(maxServiceFee);
        uint256 chargedServiceFee = asset.serviceFee;
        if (chargedServiceFee > maxServiceFee) {
            revert ServiceFeeExceedsUserMaximum(chargedServiceFee, maxServiceFee);
        }
        if (amount <= chargedServiceFee) revert InvalidAmount(amount);
        if (owner.length == 0 || owner.length > 29 || (owner.length == 1 && owner[0] == bytes1(0x04))) {
            revert InvalidPrincipal(owner);
        }
        _claimWithdrawalTransaction();

        withdrawalId = nextWithdrawalId++;
        _withdrawals[withdrawalId] = StoredWithdrawal({
            assetId: assetId,
            requester: msg.sender,
            exists: true,
            // Values are checked against type(uint128).max above.
            // forge-lint: disable-next-line(unsafe-typecast)
            amount: uint128(amount),
            // forge-lint: disable-next-line(unsafe-typecast)
            maxServiceFee: uint128(maxServiceFee),
            // The configured service fee is stored as uint128.
            // forge-lint: disable-next-line(unsafe-typecast)
            chargedServiceFee: uint128(chargedServiceFee),
            owner: owner,
            subaccount: subaccount
        });
        if (!asset.token.transferFrom(msg.sender, address(this), amount)) revert TokenTransferFailed();
        asset.token.bridgeBurn(amount);
        emit WithdrawalCommitted(
            withdrawalId,
            assetId,
            msg.sender,
            amount,
            maxServiceFee,
            chargedServiceFee,
            amount - chargedServiceFee,
            owner,
            subaccount
        );
    }

    function assetSnapshot(bytes32 assetId) external view override returns (AssetSnapshot memory) {
        AssetState storage asset = _asset(assetId);
        (uint64 windowStartedAt, uint256 consumed) = _currentWindow(asset);
        return AssetSnapshot({
            token: address(asset.token),
            assetEpoch: asset.assetEpoch,
            serviceFee: asset.serviceFee,
            minServiceFee: asset.minServiceFee,
            maxServiceFee: asset.maxServiceFee,
            perDepositLimit: asset.perDepositLimit,
            mintWindowLimit: asset.mintWindowLimit,
            mintWindowDuration: asset.mintWindowDuration,
            mintWindowStartedAt: windowStartedAt,
            mintedInWindow: consumed,
            depositMintsPaused: asset.depositMintsPaused,
            withdrawalsPaused: asset.withdrawalsPaused
        });
    }

    function tokenForAsset(bytes32 assetId) external view override returns (address) {
        return address(_asset(assetId).token);
    }

    function isDepositProcessed(bytes32 assetId, bytes32 depositId) external view override returns (bool) {
        _asset(assetId);
        return _processedDeposits[assetId][depositId];
    }

    function getWithdrawal(uint256 withdrawalId) external view override returns (Withdrawal memory) {
        StoredWithdrawal storage stored = _withdrawals[withdrawalId];
        if (!stored.exists) {
            return Withdrawal(bytes32(0), address(0), 0, 0, 0, 0, bytes(""), bytes32(0), WithdrawalStatus.None);
        }
        return Withdrawal({
            assetId: stored.assetId,
            requester: stored.requester,
            amount: stored.amount,
            maxServiceFee: stored.maxServiceFee,
            chargedServiceFee: stored.chargedServiceFee,
            amountOut: uint256(stored.amount) - stored.chargedServiceFee,
            owner: stored.owner,
            subaccount: stored.subaccount,
            status: WithdrawalStatus.Committed
        });
    }

    function pauseAssetDepositMints(bytes32 assetId) external override onlyRuntimeAdministrator {
        AssetState storage asset = _asset(assetId);
        if (asset.depositMintsPaused) return;
        asset.depositMintsPaused = true;
        _advanceAssetEpoch(assetId, asset);
        emit AssetDepositMintsPaused(assetId, msg.sender);
    }

    function pauseAssetWithdrawals(bytes32 assetId) external override onlyRuntimeAdministrator {
        AssetState storage asset = _asset(assetId);
        if (asset.withdrawalsPaused) return;
        asset.withdrawalsPaused = true;
        emit AssetWithdrawalsPaused(assetId, msg.sender);
    }

    function pauseAllDepositMints() external override onlyRuntimeAdministrator {
        if (globalDepositMintsPaused) return;
        globalDepositMintsPaused = true;
        _bridgeSignerAtLastGlobalPause = bridgeSigner;
        uint256 previousEpoch = globalEpoch;
        globalEpoch = MintAuthorizationPolicy.nextEpoch(previousEpoch);
        emit GlobalEpochChanged(msg.sender, previousEpoch, globalEpoch);
        emit GlobalDepositMintsPaused(msg.sender);
    }

    function pauseAllWithdrawals() external override onlyRuntimeAdministrator {
        if (globalWithdrawalsPaused) return;
        globalWithdrawalsPaused = true;
        emit GlobalWithdrawalsPaused(msg.sender);
    }

    function unpauseAssetDepositMints(bytes32 assetId) external override onlyBaseAdminTimelock {
        AssetState storage asset = _asset(assetId);
        if (!asset.depositMintsPaused) return;
        asset.depositMintsPaused = false;
        _advanceAssetEpoch(assetId, asset);
        emit AssetDepositMintsUnpaused(assetId, msg.sender);
    }

    function unpauseAssetWithdrawals(bytes32 assetId) external override onlyBaseAdminTimelock {
        AssetState storage asset = _asset(assetId);
        if (!asset.withdrawalsPaused) return;
        asset.withdrawalsPaused = false;
        emit AssetWithdrawalsUnpaused(assetId, msg.sender);
    }

    function unpauseAllDepositMints() external override onlyBaseAdminTimelock {
        if (!globalDepositMintsPaused) return;
        if (_bridgeSignerAtLastGlobalPause != address(0) && _bridgeSignerAtLastGlobalPause == bridgeSigner) {
            revert BridgeSignerRotationRequired();
        }
        globalDepositMintsPaused = false;
        emit GlobalDepositMintsUnpaused(msg.sender);
    }

    function unpauseAllWithdrawals() external override onlyBaseAdminTimelock {
        if (!globalWithdrawalsPaused) return;
        globalWithdrawalsPaused = false;
        emit GlobalWithdrawalsUnpaused(msg.sender);
    }

    function setAssetServiceFee(bytes32 assetId, uint256 newServiceFee) external override onlyRuntimeAdministrator {
        AssetState storage asset = _asset(assetId);
        if (!BridgeAdministration.serviceFeeIsValid(newServiceFee, asset.minServiceFee, asset.maxServiceFee)) {
            revert InvalidServiceFee(newServiceFee, asset.maxServiceFee);
        }
        uint256 previousFee = asset.serviceFee;
        if (previousFee == newServiceFee) return;
        // The configured maximum service fee is stored as uint128.
        // forge-lint: disable-next-line(unsafe-typecast)
        asset.serviceFee = uint128(newServiceFee);
        emit AssetServiceFeeChanged(assetId, msg.sender, previousFee, newServiceFee);
    }

    function rotateBridgeSigner(address newSigner) external override onlyBaseAdminTimelock {
        address previousSigner = bridgeSigner;
        if (newSigner == previousSigner) return;
        if (_retiredBridgeSigners[newSigner]) revert BridgeSignerAlreadyRetired(newSigner);
        _validateRoleSet(newSigner, runtimeAdministrator, baseAdminTimelock);
        _retiredBridgeSigners[previousSigner] = true;
        bridgeSigner = newSigner;
        uint256 previousEpoch = globalEpoch;
        globalEpoch = previousEpoch + 1;
        emit BridgeSignerChanged(previousSigner, newSigner);
        emit GlobalEpochChanged(msg.sender, previousEpoch, globalEpoch);
    }

    function rotateRuntimeAdministrator(address newAdministrator) external override onlyBaseAdminTimelock {
        address previousAdministrator = runtimeAdministrator;
        if (newAdministrator == previousAdministrator) return;
        _validateRoleSet(bridgeSigner, newAdministrator, baseAdminTimelock);
        runtimeAdministrator = newAdministrator;
        emit RuntimeAdministratorChanged(previousAdministrator, newAdministrator);
    }

    function _asset(bytes32 assetId) private view returns (AssetState storage asset) {
        asset = _assets[assetId];
        if (!asset.exists) revert UnknownAsset(assetId);
    }

    function _currentWindow(AssetState storage asset) private view returns (uint64 startedAt, uint256 consumed) {
        startedAt = asset.mintWindowStartedAt;
        consumed = asset.mintedInWindow;
        // Consensus timestamp defines the fixed mint-window boundary.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp >= uint256(startedAt) + asset.mintWindowDuration) {
            startedAt = uint64(block.timestamp);
            consumed = 0;
        }
    }

    function _advanceAssetEpoch(bytes32 assetId, AssetState storage asset) private {
        uint256 previousEpoch = asset.assetEpoch;
        asset.assetEpoch = previousEpoch + 1;
        emit AssetEpochChanged(assetId, msg.sender, previousEpoch, asset.assetEpoch);
    }

    function _requireU128(uint256 value) private pure {
        if (value > type(uint128).max) revert ValueExceedsU128(value);
    }

    function _revertRejectedMint(
        MintAuthorizationPolicy.RejectReason reason,
        MintAuthorization calldata authorization,
        uint256 windowAvailable,
        uint256 maximumServiceFee
    ) private view {
        if (reason == MintAuthorizationPolicy.RejectReason.None) return;
        if (reason == MintAuthorizationPolicy.RejectReason.Expired) {
            revert MintAuthorizationExpired(block.timestamp, authorization.deadline);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.DeadlineTooFar) {
            revert MintAuthorizationDeadlineTooFar(authorization.deadline, block.timestamp + 15 minutes);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.ZeroRecipient) revert ZeroAddress();
        if (reason == MintAuthorizationPolicy.RejectReason.InvalidRecipient) {
            revert InvalidMintRecipient(authorization.recipient);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.GrossExceedsU128) {
            revert ValueExceedsU128(authorization.grossAmount);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.MaximumFeeExceedsU128) {
            revert ValueExceedsU128(authorization.maxServiceFee);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.ChargedFeeExceedsU128) {
            revert ValueExceedsU128(authorization.chargedServiceFee);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.Processed) {
            revert DepositAlreadyProcessed(authorization.assetId, authorization.depositId);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.ProtocolFeeExceeded) {
            revert InvalidServiceFee(authorization.chargedServiceFee, maximumServiceFee);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.UserFeeExceeded) {
            revert ServiceFeeExceedsUserMaximum(authorization.chargedServiceFee, authorization.maxServiceFee);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.InvalidAmount) {
            revert InvalidAmount(authorization.grossAmount);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.PerDepositLimitExceeded) {
            revert DepositMintLimitExceeded(
                authorization.grossAmount - authorization.chargedServiceFee,
                _assets[authorization.assetId].perDepositLimit
            );
        }
        if (reason == MintAuthorizationPolicy.RejectReason.WindowLimitExceeded) {
            revert MintWindowLimitExceeded(authorization.grossAmount - authorization.chargedServiceFee, windowAvailable);
        }
        if (reason == MintAuthorizationPolicy.RejectReason.TimestampExceedsU64) {
            revert BlockTimestampExceedsU64(block.timestamp);
        }
        revert InvalidMintAuthorizationSignature();
    }

    function _validateRoleSet(address signer, address administrator, address timelock) private pure {
        if (!BridgeAdministration.rolesAreNonzero(signer, administrator, timelock)) revert ZeroAddress();
        if (!BridgeAdministration.rolesAreDistinct(signer, administrator, timelock)) {
            revert RoleAddressesMustDiffer();
        }
    }

    function _validateTimelockCandidate(address candidate) private view {
        if (candidate.code.length == 0) revert TimelockCandidateHasNoCode(candidate);
        bytes32 actualCodeHash = candidate.codehash;
        if (actualCodeHash != approvedTimelockRuntimeCodeHash) {
            revert TimelockCandidateCodeHashMismatch(candidate, actualCodeHash, approvedTimelockRuntimeCodeHash);
        }
        IMultiTokenTimelockCandidate timelock = IMultiTokenTimelockCandidate(candidate);
        try timelock.getMinDelay() returns (uint256 delay) {
            if (delay < DeploymentPolicy.MINIMUM_TIMELOCK_DELAY) {
                revert TimelockCandidateDelayTooShort(candidate, delay, DeploymentPolicy.MINIMUM_TIMELOCK_DELAY);
            }
            if (delay > MAXIMUM_TIMELOCK_DELAY) {
                revert TimelockCandidateDelayTooLong(candidate, delay, MAXIMUM_TIMELOCK_DELAY);
            }
        } catch {
            revert TimelockCandidateIntrospectionFailed(candidate);
        }
        try timelock.hasRole(bytes32(0), candidate) returns (bool hasSelfAdmin) {
            if (!hasSelfAdmin) revert TimelockCandidateMissingSelfAdmin(candidate);
        } catch {
            revert TimelockCandidateIntrospectionFailed(candidate);
        }
        _validateTimelockRole(timelock, candidate, bytes32(0), candidate);
        _validateTimelockRole(timelock, candidate, PROPOSER_ROLE, address(0));
        _validateTimelockRole(timelock, candidate, CANCELLER_ROLE, address(0));
        _validateTimelockRole(timelock, candidate, EXECUTOR_ROLE, address(0));
        try timelock.pendingOperationCount() returns (uint256 count) {
            if (count != 0) revert TimelockCandidateHasPendingOperations(candidate, count);
        } catch {
            revert TimelockCandidateIntrospectionFailed(candidate);
        }
    }

    function _validateTimelockRole(
        IMultiTokenTimelockCandidate timelock,
        address candidate,
        bytes32 role,
        address requiredMember
    ) private view {
        address member;
        try timelock.roleMember(role) returns (address candidateMember) {
            member = candidateMember;
        } catch {
            revert TimelockCandidateIntrospectionFailed(candidate);
        }
        bool memberHasRole;
        bool roleIsOpen;
        try timelock.hasRole(role, member) returns (bool hasRole) {
            memberHasRole = hasRole;
        } catch {
            revert TimelockCandidateIntrospectionFailed(candidate);
        }
        if (role != bytes32(0)) {
            try timelock.hasRole(role, address(0)) returns (bool isOpen) {
                roleIsOpen = isOpen;
            } catch {
                revert TimelockCandidateIntrospectionFailed(candidate);
            }
        }
        if (!BridgeAdministration.timelockRoleIsClosed(member, requiredMember, memberHasRole, roleIsOpen)) {
            revert TimelockCandidateInvalidRoleMember(candidate, role, member);
        }
    }

    function _claimWithdrawalTransaction() private {
        bytes32 slot = WITHDRAWAL_TRANSACTION_SLOT;
        uint256 claimed;
        assembly ("memory-safe") {
            claimed := tload(slot)
        }
        if (claimed != 0) revert MultipleWithdrawalsInTransaction();
        assembly ("memory-safe") {
            tstore(slot, 1)
        }
    }
}
