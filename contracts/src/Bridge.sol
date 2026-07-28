// contracts/src: implement Base asset flows and split immediate safety controls from timelocked administration.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BSNS} from "./BSNS.sol";
import {IBSNS} from "./interfaces/IBSNS.sol";
import {IBridge} from "./interfaces/IBridge.sol";
import {BridgeAdministration} from "./libraries/BridgeAdministration.sol";
import {MintAccounting} from "./libraries/MintAccounting.sol";
import {ECDSA} from "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import {EIP712} from "@openzeppelin/contracts/utils/cryptography/EIP712.sol";

interface ITimelockCandidate {
    function getMinDelay() external view returns (uint256);
    function hasRole(bytes32 role, address account) external view returns (bool);
    function roleMember(bytes32 role) external view returns (address);
    function pendingOperationCount() external view returns (uint256);
}

/// @notice Phase 1E Base implementation whose concrete ABI is checked against the frozen interface snapshot.
contract Bridge is IBridge, EIP712 {
    uint256 private constant MINIMUM_TIMELOCK_DELAY = 24 hours;
    uint256 private constant MAXIMUM_TIMELOCK_DELAY = 30 days;
    uint64 private constant MINIMUM_MINT_WINDOW_DURATION = 1 hours;
    uint64 private constant MAXIMUM_MINT_WINDOW_DURATION = 30 days;
    bytes32 private constant PROPOSER_ROLE = keccak256("PROPOSER_ROLE");
    bytes32 private constant CANCELLER_ROLE = keccak256("CANCELLER_ROLE");
    bytes32 private constant EXECUTOR_ROLE = keccak256("EXECUTOR_ROLE");
    bytes32 private constant WITHDRAWAL_TRANSACTION_SLOT = keccak256("kinic.bridge.withdrawal.transaction");
    bytes32 private constant MINT_AUTHORIZATION_TYPEHASH = keccak256(
        "MintAuthorization(bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 authorizationEpoch)"
    );
    bytes32 public immutable override approvedTimelockRuntimeCodeHash;
    IBSNS public immutable override bsns;
    uint256 public immutable override MAX_SERVICE_FEE;

    address public override bridgeSigner;
    uint256 public override mintAuthorizationEpoch = 1;
    address public override runtimeAdministrator;
    address public override baseAdminTimelock;
    uint256 public override serviceFee;
    uint256 public immutable override perDepositLimit;
    uint256 public immutable override mintWindowLimit;
    uint64 public immutable override mintWindowDuration;
    uint64 public override mintWindowStartedAt;
    uint256 public override mintedInWindow;
    bool public override depositMintsPaused = true;
    bool public override withdrawalsPaused = true;
    uint256 public override nextWithdrawalId = 1;

    mapping(bytes32 depositId => bool processed) private _processedDeposits;
    mapping(uint256 withdrawalId => IBridge.Withdrawal withdrawal) private _withdrawals;

    modifier onlyRuntimeAdministrator() {
        if (msg.sender != runtimeAdministrator) {
            revert IBridge.UnauthorizedRuntimeAdministrator(msg.sender);
        }
        _;
    }

    modifier onlyBaseAdminTimelock() {
        if (msg.sender != baseAdminTimelock) {
            revert IBridge.UnauthorizedBaseAdmin(msg.sender);
        }
        _;
    }

    modifier whenDepositMintsActive() {
        if (depositMintsPaused) {
            revert IBridge.DepositMintsArePaused();
        }
        _;
    }

    modifier whenWithdrawalsActive() {
        if (withdrawalsPaused) {
            revert IBridge.WithdrawalsArePaused();
        }
        _;
    }

    constructor(
        string memory tokenName,
        string memory tokenSymbol,
        uint8 tokenDecimals,
        address initialBridgeSigner,
        address initialRuntimeAdministrator,
        address initialBaseAdminTimelock,
        bytes32 initialApprovedTimelockRuntimeCodeHash,
        uint256 initialPerDepositLimit,
        uint256 initialMintWindowLimit,
        uint64 initialMintWindowDuration,
        uint256 maxServiceFee,
        uint256 initialServiceFee
    ) EIP712("KINIC Bridge", "1") {
        if (!BridgeAdministration.rolesAreNonzero(
                initialBridgeSigner, initialRuntimeAdministrator, initialBaseAdminTimelock
            )) {
            revert IBridge.ZeroAddress();
        }
        if (!BridgeAdministration.rolesAreDistinct(
                initialBridgeSigner, initialRuntimeAdministrator, initialBaseAdminTimelock
            )) {
            revert IBridge.RoleAddressesMustDiffer();
        }
        if (
            !BridgeAdministration.limitsAreNonzero(
                    initialPerDepositLimit, initialMintWindowLimit, initialMintWindowDuration
                ) || maxServiceFee == 0
        ) {
            revert IBridge.InvalidAmount(0);
        }
        if (!BridgeAdministration.valuesFitU128(initialPerDepositLimit, initialMintWindowLimit, maxServiceFee)) {
            revert IBridge.ValueExceedsU128(initialPerDepositLimit > type(uint128).max
                    ? initialPerDepositLimit
                    : initialMintWindowLimit > type(uint128).max ? initialMintWindowLimit : maxServiceFee);
        }
        if (
            initialMintWindowDuration < MINIMUM_MINT_WINDOW_DURATION
                || initialMintWindowDuration > MAXIMUM_MINT_WINDOW_DURATION
        ) {
            revert IBridge.InvalidMintWindowDuration(
                initialMintWindowDuration, MINIMUM_MINT_WINDOW_DURATION, MAXIMUM_MINT_WINDOW_DURATION
            );
        }
        if (!BridgeAdministration.serviceFeeIsValid(initialServiceFee, maxServiceFee)) {
            revert IBridge.InvalidServiceFee(initialServiceFee, maxServiceFee);
        }
        approvedTimelockRuntimeCodeHash = initialApprovedTimelockRuntimeCodeHash;
        _validateTimelockCandidate(initialBaseAdminTimelock);

        bridgeSigner = initialBridgeSigner;
        runtimeAdministrator = initialRuntimeAdministrator;
        baseAdminTimelock = initialBaseAdminTimelock;
        perDepositLimit = initialPerDepositLimit;
        mintWindowLimit = initialMintWindowLimit;
        mintWindowDuration = initialMintWindowDuration;
        mintWindowStartedAt = uint64(block.timestamp);
        MAX_SERVICE_FEE = maxServiceFee;
        serviceFee = initialServiceFee;
        bsns = new BSNS(tokenName, tokenSymbol, tokenDecimals, address(this));
    }

    function mintDepositWithAuthorization(IBridge.MintAuthorization calldata authorization, bytes calldata signature)
        external
        override
        whenDepositMintsActive
    {
        // The signed authorization is deliberately valid at the exact deadline.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp > authorization.deadline) {
            revert IBridge.MintAuthorizationExpired(block.timestamp, authorization.deadline);
        }
        if (authorization.authorizationEpoch != mintAuthorizationEpoch) {
            revert IBridge.MintAuthorizationEpochMismatch(authorization.authorizationEpoch, mintAuthorizationEpoch);
        }
        bytes32 digest = _mintAuthorizationDigest(authorization);
        (address recovered, ECDSA.RecoverError error,) = ECDSA.tryRecoverCalldata(digest, signature);
        if (error != ECDSA.RecoverError.NoError || recovered != bridgeSigner) {
            revert IBridge.InvalidMintAuthorizationSignature();
        }
        _rollMintWindowIfExpired();
        uint256 mintAmount = _validateAndMarkDeposit(authorization);
        mintedInWindow = _consumeMintWindow(mintAmount);
        bsns.bridgeMint(authorization.recipient, mintAmount);
        emit IBridge.DepositMinted(
            authorization.depositId,
            authorization.recipient,
            digest,
            authorization.grossAmount,
            authorization.chargedServiceFee,
            mintAmount
        );
    }

    function createWithdrawal(uint256 amount, uint256 maxServiceFee, bytes calldata owner, bytes32 subaccount)
        external
        override
        whenWithdrawalsActive
        returns (uint256 withdrawalId)
    {
        if (!BridgeAdministration.valueFitsU128(amount)) {
            revert IBridge.ValueExceedsU128(amount);
        }
        if (!BridgeAdministration.valueFitsU128(maxServiceFee)) {
            revert IBridge.ValueExceedsU128(maxServiceFee);
        }
        if (amount == 0) {
            revert IBridge.InvalidAmount(amount);
        }
        uint256 chargedServiceFee = serviceFee;
        if (chargedServiceFee > maxServiceFee) {
            revert IBridge.ServiceFeeExceedsUserMaximum(chargedServiceFee, maxServiceFee);
        }
        if (amount <= chargedServiceFee) {
            revert IBridge.InvalidAmount(amount);
        }
        if (owner.length == 0 || owner.length > 29 || (owner.length == 1 && owner[0] == bytes1(0x04))) {
            revert IBridge.InvalidPrincipal(owner);
        }

        _claimWithdrawalTransaction();

        withdrawalId = nextWithdrawalId;
        nextWithdrawalId = withdrawalId + 1;
        IBridge.Withdrawal storage withdrawal = _withdrawals[withdrawalId];
        withdrawal.requester = msg.sender;
        withdrawal.amount = amount;
        withdrawal.maxServiceFee = maxServiceFee;
        withdrawal.chargedServiceFee = chargedServiceFee;
        withdrawal.amountOut = amount - chargedServiceFee;
        withdrawal.owner = owner;
        withdrawal.subaccount = subaccount;
        withdrawal.status = IBridge.WithdrawalStatus.Committed;

        if (!bsns.transferFrom(msg.sender, address(this), amount)) {
            revert IBridge.TokenTransferFailed();
        }
        bsns.bridgeBurn(amount);
        emit IBridge.WithdrawalCommitted(
            withdrawalId, msg.sender, amount, maxServiceFee, chargedServiceFee, withdrawal.amountOut, owner, subaccount
        );
    }

    function bridgeSnapshot() external view override returns (IBridge.BridgeSnapshot memory) {
        return IBridge.BridgeSnapshot({
            blockNumber: block.number,
            blockTimestamp: block.timestamp,
            bridgeSigner: bridgeSigner,
            mintAuthorizationEpoch: mintAuthorizationEpoch,
            serviceFee: serviceFee,
            maxServiceFee: MAX_SERVICE_FEE,
            perDepositLimit: perDepositLimit,
            mintWindowLimit: mintWindowLimit,
            mintWindowDuration: mintWindowDuration,
            mintWindowStartedAt: mintWindowStartedAt,
            mintedInWindow: mintedInWindow,
            depositMintsPaused: depositMintsPaused,
            withdrawalsPaused: withdrawalsPaused
        });
    }

    function pauseDepositMints() external override onlyRuntimeAdministrator {
        if (depositMintsPaused) {
            return;
        }
        depositMintsPaused = true;
        _incrementMintAuthorizationEpoch();
        emit IBridge.DepositMintsPaused(msg.sender);
    }

    function pauseWithdrawals() external override onlyRuntimeAdministrator {
        if (withdrawalsPaused) {
            return;
        }
        withdrawalsPaused = true;
        emit IBridge.WithdrawalsPaused(msg.sender);
    }

    function unpauseDepositMints() external override onlyBaseAdminTimelock {
        if (!depositMintsPaused) {
            return;
        }
        depositMintsPaused = false;
        emit IBridge.DepositMintsUnpaused(msg.sender);
    }

    function unpauseWithdrawals() external override onlyBaseAdminTimelock {
        if (!withdrawalsPaused) {
            return;
        }
        withdrawalsPaused = false;
        emit IBridge.WithdrawalsUnpaused(msg.sender);
    }

    function setServiceFee(uint256 newServiceFee) external override onlyRuntimeAdministrator {
        if (!BridgeAdministration.serviceFeeIsValid(newServiceFee, MAX_SERVICE_FEE)) {
            revert IBridge.InvalidServiceFee(newServiceFee, MAX_SERVICE_FEE);
        }
        uint256 previousFee = serviceFee;
        if (newServiceFee == previousFee) {
            return;
        }
        serviceFee = newServiceFee;
        emit IBridge.ServiceFeeChanged(msg.sender, previousFee, newServiceFee);
    }

    function rotateBridgeSigner(address newSigner) external override onlyBaseAdminTimelock {
        address previousSigner = bridgeSigner;
        if (newSigner == previousSigner) {
            return;
        }
        _validateRoleSet(newSigner, runtimeAdministrator, baseAdminTimelock);
        bridgeSigner = newSigner;
        _incrementMintAuthorizationEpoch();
        emit IBridge.BridgeSignerChanged(previousSigner, newSigner);
    }

    function rotateRuntimeAdministrator(address newAdministrator) external override onlyBaseAdminTimelock {
        address previousAdministrator = runtimeAdministrator;
        if (newAdministrator == previousAdministrator) {
            return;
        }
        _validateRoleSet(bridgeSigner, newAdministrator, baseAdminTimelock);
        runtimeAdministrator = newAdministrator;
        emit IBridge.RuntimeAdministratorChanged(previousAdministrator, newAdministrator);
    }

    function rotateBaseAdminTimelock(address newTimelock) external override onlyBaseAdminTimelock {
        address previousTimelock = baseAdminTimelock;
        if (newTimelock == previousTimelock) {
            return;
        }
        _validateRoleSet(bridgeSigner, runtimeAdministrator, newTimelock);
        _validateTimelockCandidate(newTimelock);
        baseAdminTimelock = newTimelock;
        emit IBridge.BaseAdminTimelockChanged(previousTimelock, newTimelock);
    }

    function isDepositProcessed(bytes32 depositId) external view override returns (bool) {
        return _processedDeposits[depositId];
    }

    function getWithdrawal(uint256 withdrawalId) external view override returns (IBridge.Withdrawal memory) {
        return _withdrawals[withdrawalId];
    }

    function _validateRoleSet(
        address candidateBridgeSigner,
        address candidateRuntimeAdministrator,
        address candidateTimelock
    ) private pure {
        if (!BridgeAdministration.rolesAreNonzero(
                candidateBridgeSigner, candidateRuntimeAdministrator, candidateTimelock
            )) {
            revert IBridge.ZeroAddress();
        }
        if (!BridgeAdministration.rolesAreDistinct(
                candidateBridgeSigner, candidateRuntimeAdministrator, candidateTimelock
            )) {
            revert IBridge.RoleAddressesMustDiffer();
        }
    }

    function _validateTimelockCandidate(address candidate) private view {
        if (candidate.code.length == 0) {
            revert IBridge.TimelockCandidateHasNoCode(candidate);
        }
        bytes32 expectedCodeHash = approvedTimelockRuntimeCodeHash;
        bytes32 actualCodeHash = candidate.codehash;
        if (actualCodeHash != expectedCodeHash) {
            revert IBridge.TimelockCandidateCodeHashMismatch(candidate, actualCodeHash, expectedCodeHash);
        }

        uint256 delay;
        bool selfAdmin;
        try ITimelockCandidate(candidate).getMinDelay() returns (uint256 candidateDelay) {
            delay = candidateDelay;
        } catch {
            revert IBridge.TimelockCandidateIntrospectionFailed(candidate);
        }
        try ITimelockCandidate(candidate).hasRole(bytes32(0), candidate) returns (bool candidateSelfAdmin) {
            selfAdmin = candidateSelfAdmin;
        } catch {
            revert IBridge.TimelockCandidateIntrospectionFailed(candidate);
        }
        if (delay < MINIMUM_TIMELOCK_DELAY) {
            revert IBridge.TimelockCandidateDelayTooShort(candidate, delay, MINIMUM_TIMELOCK_DELAY);
        }
        if (!BridgeAdministration.timelockDelayIsValid(delay, MINIMUM_TIMELOCK_DELAY, MAXIMUM_TIMELOCK_DELAY)) {
            revert IBridge.TimelockCandidateDelayTooLong(candidate, delay, MAXIMUM_TIMELOCK_DELAY);
        }
        if (!selfAdmin) {
            revert IBridge.TimelockCandidateMissingSelfAdmin(candidate);
        }
        _validateTimelockRole(candidate, bytes32(0), candidate);
        _validateTimelockRole(candidate, PROPOSER_ROLE, address(0));
        _validateTimelockRole(candidate, CANCELLER_ROLE, address(0));
        _validateTimelockRole(candidate, EXECUTOR_ROLE, address(0));
        uint256 pending;
        try ITimelockCandidate(candidate).pendingOperationCount() returns (uint256 candidatePending) {
            pending = candidatePending;
        } catch {
            revert IBridge.TimelockCandidateIntrospectionFailed(candidate);
        }
        if (!BridgeAdministration.timelockHasNoPendingOperations(pending)) {
            revert IBridge.TimelockCandidateHasPendingOperations(candidate, pending);
        }
    }

    function _validateTimelockRole(address candidate, bytes32 role, address requiredMember) private view {
        address member;
        try ITimelockCandidate(candidate).roleMember(role) returns (address candidateMember) {
            member = candidateMember;
        } catch {
            revert IBridge.TimelockCandidateIntrospectionFailed(candidate);
        }
        bool memberHasRole = ITimelockCandidate(candidate).hasRole(role, member);
        bool roleIsOpen = role != bytes32(0) && ITimelockCandidate(candidate).hasRole(role, address(0));
        if (!BridgeAdministration.timelockRoleIsClosed(member, requiredMember, memberHasRole, roleIsOpen)) {
            revert IBridge.TimelockCandidateInvalidRoleMember(candidate, role, member);
        }
    }

    function _validateAndMarkDeposit(IBridge.MintAuthorization calldata request) private returns (uint256 mintAmount) {
        if (request.recipient == address(0)) {
            revert IBridge.ZeroAddress();
        }
        if (request.recipient == address(this) || request.recipient == address(bsns)) {
            revert IBridge.InvalidMintRecipient(request.recipient);
        }
        if (!BridgeAdministration.valueFitsU128(request.grossAmount)) {
            revert IBridge.ValueExceedsU128(request.grossAmount);
        }
        if (!BridgeAdministration.valueFitsU128(request.maxServiceFee)) {
            revert IBridge.ValueExceedsU128(request.maxServiceFee);
        }
        if (!BridgeAdministration.valueFitsU128(request.chargedServiceFee)) {
            revert IBridge.ValueExceedsU128(request.chargedServiceFee);
        }
        if (_processedDeposits[request.depositId]) {
            revert IBridge.DepositAlreadyProcessed(request.depositId);
        }
        if (request.chargedServiceFee > MAX_SERVICE_FEE) {
            revert IBridge.InvalidServiceFee(request.chargedServiceFee, MAX_SERVICE_FEE);
        }
        if (request.chargedServiceFee > request.maxServiceFee) {
            revert IBridge.ServiceFeeExceedsUserMaximum(request.chargedServiceFee, request.maxServiceFee);
        }

        if (request.grossAmount <= request.chargedServiceFee) {
            revert IBridge.InvalidAmount(request.grossAmount);
        }
        mintAmount = MintAccounting.netAmount(request.grossAmount, request.chargedServiceFee);
        if (mintAmount > perDepositLimit) {
            revert IBridge.DepositMintLimitExceeded(mintAmount, perDepositLimit);
        }
        // Mark during batch validation so duplicate IDs fail; a later revert rolls the mark back.
        _processedDeposits[request.depositId] = true;
    }

    function _mintAuthorizationDigest(IBridge.MintAuthorization calldata authorization) private view returns (bytes32) {
        return _hashTypedDataV4(
            keccak256(
                abi.encode(
                    MINT_AUTHORIZATION_TYPEHASH,
                    authorization.depositId,
                    authorization.recipient,
                    authorization.grossAmount,
                    authorization.maxServiceFee,
                    authorization.chargedServiceFee,
                    authorization.deadline,
                    authorization.authorizationEpoch
                )
            )
        );
    }

    function _incrementMintAuthorizationEpoch() private {
        uint256 previousEpoch = mintAuthorizationEpoch;
        uint256 nextEpoch = previousEpoch + 1;
        mintAuthorizationEpoch = nextEpoch;
        emit IBridge.MintAuthorizationEpochChanged(msg.sender, previousEpoch, nextEpoch);
    }

    function _claimWithdrawalTransaction() private {
        bytes32 slot = WITHDRAWAL_TRANSACTION_SLOT;
        uint256 claimed;
        assembly ("memory-safe") {
            claimed := tload(slot)
        }
        if (!BridgeAdministration.withdrawalClaimAllowed(claimed != 0)) {
            revert IBridge.MultipleWithdrawalsInTransaction();
        }
        assembly ("memory-safe") {
            tstore(slot, 1)
        }
    }

    function _consumeMintWindow(uint256 requested) private view returns (uint256 nextConsumed) {
        (bool accepted, uint256 candidate, uint256 available) =
            MintAccounting.tryConsumeWindow(mintedInWindow, requested, mintWindowLimit);
        if (!accepted) {
            revert IBridge.MintWindowLimitExceeded(requested, available);
        }
        return candidate;
    }

    function _rollMintWindowIfExpired() private {
        // Fixed windows intentionally use Base block time as their on-chain clock.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp >= uint256(mintWindowStartedAt) + uint256(mintWindowDuration)) {
            mintWindowStartedAt = uint64(block.timestamp);
            mintedInWindow = 0;
        }
    }
}
