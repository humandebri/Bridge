// contracts/src: implement Base asset flows and split immediate safety controls from timelocked administration.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BSNS} from "./BSNS.sol";
import {IBSNS} from "./interfaces/IBSNS.sol";
import {IBridge} from "./interfaces/IBridge.sol";
import {BridgeAdministration} from "./libraries/BridgeAdministration.sol";
import {MintAccounting} from "./libraries/MintAccounting.sol";
import {WithdrawalAccounting} from "./libraries/WithdrawalAccounting.sol";

/// @notice Phase 1E Base implementation whose concrete ABI is checked against the frozen interface snapshot.
contract Bridge is IBridge {
    uint256 private constant MAX_BATCH_SIZE = 4;
    IBSNS public immutable override bsns;
    uint256 public immutable override MAX_SERVICE_FEE;

    address public override bridgeSigner;
    address public override runtimeAdministrator;
    address public override baseAdminTimelock;
    uint256 public override serviceFee;
    uint256 public immutable override perDepositLimit;
    uint256 public immutable override mintWindowLimit;
    uint64 public immutable override mintWindowDuration;
    uint64 public override mintWindowStartedAt;
    uint256 public override mintedInWindow;
    bool public override depositMintsPaused;
    bool public override withdrawalsPaused;
    uint256 public override nextWithdrawalId = 1;

    mapping(bytes32 depositId => bool processed) private _processedDeposits;
    mapping(uint256 withdrawalId => IBridge.Withdrawal withdrawal) private _withdrawals;
    mapping(uint256 ledgerBlockIndex => uint256 withdrawalId) private _withdrawalIdByLedgerBlockIndex;

    modifier onlyBridgeSigner() {
        if (msg.sender != bridgeSigner) {
            revert IBridge.UnauthorizedBridgeSigner(msg.sender);
        }
        _;
    }

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
        uint256 initialPerDepositLimit,
        uint256 initialMintWindowLimit,
        uint64 initialMintWindowDuration,
        uint256 maxServiceFee,
        uint256 initialServiceFee
    ) {
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
        if (!BridgeAdministration.serviceFeeIsValid(initialServiceFee, maxServiceFee)) {
            revert IBridge.InvalidServiceFee(initialServiceFee, maxServiceFee);
        }

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

    function mintDeposit(IBridge.DepositMintRequest calldata request)
        external
        override
        onlyBridgeSigner
        whenDepositMintsActive
    {
        _rollMintWindowIfExpired();
        uint256 mintAmount = _validateAndMarkDeposit(request);
        mintedInWindow = _consumeMintWindow(mintAmount);
        bsns.bridgeMint(request.recipient, mintAmount);
        emit IBridge.DepositMinted(
            request.depositId, request.recipient, request.grossAmount, request.chargedServiceFee, mintAmount
        );
    }

    function mintDeposits(IBridge.DepositMintRequest[] calldata requests)
        external
        override
        onlyBridgeSigner
        whenDepositMintsActive
    {
        uint256 requestCount = requests.length;
        _validateBatchSize(requestCount);

        _rollMintWindowIfExpired();
        uint256[] memory mintAmounts = new uint256[](requestCount);
        uint256 batchMintAmount;
        for (uint256 index; index < requestCount; ++index) {
            uint256 mintAmount = _validateAndMarkDeposit(requests[index]);
            mintAmounts[index] = mintAmount;
            batchMintAmount += mintAmount;
        }

        mintedInWindow = _consumeMintWindow(batchMintAmount);
        for (uint256 index; index < requestCount; ++index) {
            IBridge.DepositMintRequest calldata request = requests[index];
            uint256 mintAmount = mintAmounts[index];
            bsns.bridgeMint(request.recipient, mintAmount);
            emit IBridge.DepositMinted(
                request.depositId, request.recipient, request.grossAmount, request.chargedServiceFee, mintAmount
            );
        }
    }

    function createWithdrawal(uint256 amount, uint256 minAmountOut, bytes calldata owner, bytes32 subaccount)
        external
        override
        whenWithdrawalsActive
        returns (uint256 withdrawalId)
    {
        if (amount == 0) {
            revert IBridge.InvalidAmount(amount);
        }
        if (minAmountOut == 0 || minAmountOut > amount) {
            revert IBridge.InvalidMinAmountOut(minAmountOut, amount);
        }
        if (owner.length == 0 || owner.length > 29 || (owner.length == 1 && owner[0] == bytes1(0x04))) {
            revert IBridge.InvalidPrincipal(owner);
        }

        withdrawalId = nextWithdrawalId;
        nextWithdrawalId = withdrawalId + 1;
        IBridge.Withdrawal storage withdrawal = _withdrawals[withdrawalId];
        withdrawal.requester = msg.sender;
        withdrawal.amount = amount;
        withdrawal.minAmountOut = minAmountOut;
        withdrawal.owner = owner;
        withdrawal.subaccount = subaccount;
        withdrawal.status = IBridge.WithdrawalStatus.Pending;

        bsns.bridgeBurn(msg.sender, amount);
        emit IBridge.WithdrawalCreated(withdrawalId, msg.sender, amount, minAmountOut, owner, subaccount);
    }

    function acknowledgeRelease(
        uint256 withdrawalId,
        uint256 amountOut,
        uint256 withdrawalServiceFee,
        uint256 ledgerFee,
        uint256 ledgerBlockIndex
    ) external override onlyBridgeSigner {
        _acknowledgeRelease(withdrawalId, amountOut, withdrawalServiceFee, ledgerFee, ledgerBlockIndex);
    }

    function acknowledgeReleases(IBridge.ReleaseAcknowledgement[] calldata acknowledgements)
        external
        override
        onlyBridgeSigner
    {
        uint256 count = acknowledgements.length;
        _validateBatchSize(count);
        for (uint256 index; index < count; ++index) {
            IBridge.ReleaseAcknowledgement calldata acknowledgement = acknowledgements[index];
            _acknowledgeRelease(
                acknowledgement.withdrawalId,
                acknowledgement.amountOut,
                acknowledgement.serviceFee,
                acknowledgement.ledgerFee,
                acknowledgement.ledgerBlockIndex
            );
        }
    }

    function _acknowledgeRelease(
        uint256 withdrawalId,
        uint256 amountOut,
        uint256 withdrawalServiceFee,
        uint256 ledgerFee,
        uint256 ledgerBlockIndex
    ) private {
        IBridge.Withdrawal storage withdrawal = _withdrawals[withdrawalId];
        IBridge.WithdrawalStatus status = withdrawal.status;
        if (status == IBridge.WithdrawalStatus.None) {
            revert IBridge.WithdrawalNotFound(withdrawalId);
        }

        bool detailsMatch = status == IBridge.WithdrawalStatus.Released && withdrawal.amountOut == amountOut
            && withdrawal.serviceFee == withdrawalServiceFee && withdrawal.ledgerFee == ledgerFee
            && withdrawal.ledgerBlockIndex == ledgerBlockIndex;
        WithdrawalAccounting.ReleaseAction action = WithdrawalAccounting.releaseAction(status, detailsMatch);
        if (action == WithdrawalAccounting.ReleaseAction.Idempotent) {
            return;
        }
        if (action == WithdrawalAccounting.ReleaseAction.Reject) {
            if (status == IBridge.WithdrawalStatus.Released) {
                revert IBridge.ReleaseAcknowledgementMismatch(withdrawalId);
            }
            revert IBridge.InvalidWithdrawalStatus(withdrawalId, status);
        }

        if (!WithdrawalAccounting.feeWithinMaximum(withdrawalServiceFee, MAX_SERVICE_FEE)) {
            revert IBridge.InvalidServiceFee(withdrawalServiceFee, MAX_SERVICE_FEE);
        }
        if (!WithdrawalAccounting.settlementMatches(withdrawal.amount, amountOut, withdrawalServiceFee, ledgerFee)) {
            revert IBridge.SettlementAmountsMismatch(withdrawal.amount, amountOut, withdrawalServiceFee, ledgerFee);
        }
        if (!WithdrawalAccounting.meetsMinimum(amountOut, withdrawal.minAmountOut)) {
            revert IBridge.InvalidMinAmountOut(withdrawal.minAmountOut, amountOut);
        }

        uint256 existingWithdrawalId = _withdrawalIdByLedgerBlockIndex[ledgerBlockIndex];
        if (existingWithdrawalId != 0) {
            revert IBridge.LedgerBlockAlreadyAcknowledged(ledgerBlockIndex, existingWithdrawalId);
        }
        _withdrawalIdByLedgerBlockIndex[ledgerBlockIndex] = withdrawalId;
        withdrawal.status = IBridge.WithdrawalStatus.Released;
        withdrawal.amountOut = amountOut;
        withdrawal.serviceFee = withdrawalServiceFee;
        withdrawal.ledgerFee = ledgerFee;
        withdrawal.ledgerBlockIndex = ledgerBlockIndex;
        emit IBridge.WithdrawalReleased(withdrawalId, amountOut, withdrawalServiceFee, ledgerFee, ledgerBlockIndex);
    }

    function refundWithdrawal(uint256 withdrawalId) external override onlyBridgeSigner {
        _refundWithdrawal(withdrawalId);
    }

    function refundWithdrawals(uint256[] calldata withdrawalIds) external override onlyBridgeSigner {
        uint256 count = withdrawalIds.length;
        _validateBatchSize(count);
        for (uint256 index; index < count; ++index) {
            _refundWithdrawal(withdrawalIds[index]);
        }
    }

    function _refundWithdrawal(uint256 withdrawalId) private {
        IBridge.Withdrawal storage withdrawal = _withdrawals[withdrawalId];
        IBridge.WithdrawalStatus status = withdrawal.status;
        if (status == IBridge.WithdrawalStatus.None) {
            revert IBridge.WithdrawalNotFound(withdrawalId);
        }
        if (!WithdrawalAccounting.refundAllowed(status)) {
            revert IBridge.InvalidWithdrawalStatus(withdrawalId, status);
        }

        withdrawal.status = IBridge.WithdrawalStatus.Refunded;
        bsns.bridgeMint(withdrawal.requester, withdrawal.amount);
        emit IBridge.WithdrawalRefunded(withdrawalId, withdrawal.requester, withdrawal.amount);
    }

    function bridgeSnapshot() external view override returns (IBridge.BridgeSnapshot memory) {
        return IBridge.BridgeSnapshot({
            blockNumber: block.number,
            blockTimestamp: block.timestamp,
            bridgeSigner: bridgeSigner,
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

    function _validateBatchSize(uint256 count) private pure {
        if (count == 0) {
            revert IBridge.EmptyBatch();
        }
        if (count > MAX_BATCH_SIZE) {
            revert IBridge.BatchTooLarge(count, MAX_BATCH_SIZE);
        }
    }

    function _validateAndMarkDeposit(IBridge.DepositMintRequest calldata request) private returns (uint256 mintAmount) {
        if (request.recipient == address(0)) {
            revert IBridge.ZeroAddress();
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
