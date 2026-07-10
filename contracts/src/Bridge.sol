// contracts/src: implement Base Deposit mint authorization, deduplication, fee deduction, and flow limits.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BSNS} from "./BSNS.sol";
import {IBSNS} from "./interfaces/IBSNS.sol";
import {IBridge} from "./interfaces/IBridge.sol";
import {MintAccounting} from "./libraries/MintAccounting.sol";

/// @notice Phase 1B implementation. Withdrawal settlement and administration remain intentionally absent.
contract Bridge {
    IBSNS public immutable bsns;
    uint256 public immutable MAX_SERVICE_FEE;

    address public bridgeSigner;
    address public runtimeAdministrator;
    address public baseAdminTimelock;
    uint256 public serviceFee;
    uint256 public perDepositLimit;
    uint256 public mintWindowLimit;
    uint64 public mintWindowDuration;
    uint64 public mintWindowStartedAt;
    uint256 public mintedInWindow;
    bool public depositMintsPaused;
    bool public withdrawalsPaused;
    uint256 public nextWithdrawalId = 1;

    mapping(bytes32 depositId => bool processed) private _processedDeposits;

    modifier onlyBridgeSigner() {
        if (msg.sender != bridgeSigner) {
            revert IBridge.UnauthorizedBridgeSigner(msg.sender);
        }
        _;
    }

    modifier whenDepositMintsActive() {
        if (depositMintsPaused) {
            revert IBridge.DepositMintsArePaused();
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
        if (
            initialBridgeSigner == address(0) || initialRuntimeAdministrator == address(0)
                || initialBaseAdminTimelock == address(0)
        ) {
            revert IBridge.ZeroAddress();
        }
        if (
            initialBridgeSigner == initialRuntimeAdministrator || initialBridgeSigner == initialBaseAdminTimelock
                || initialRuntimeAdministrator == initialBaseAdminTimelock
        ) {
            revert IBridge.RoleAddressesMustDiffer();
        }
        if (
            initialPerDepositLimit == 0 || initialMintWindowLimit == 0 || initialMintWindowDuration == 0
                || maxServiceFee == 0
        ) {
            revert IBridge.InvalidAmount(0);
        }
        if (initialServiceFee > maxServiceFee) {
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

    function mintDeposit(IBridge.DepositMintRequest calldata request) external onlyBridgeSigner whenDepositMintsActive {
        _rollMintWindowIfExpired();
        uint256 mintAmount = _validateAndMarkDeposit(request);
        mintedInWindow = _consumeMintWindow(mintAmount);
        bsns.bridgeMint(request.recipient, mintAmount);
        emit IBridge.DepositMinted(request.depositId, request.recipient, request.grossAmount, serviceFee, mintAmount);
    }

    function mintDeposits(IBridge.DepositMintRequest[] calldata requests)
        external
        onlyBridgeSigner
        whenDepositMintsActive
    {
        uint256 requestCount = requests.length;
        if (requestCount == 0) {
            revert IBridge.EmptyBatch();
        }

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
                request.depositId, request.recipient, request.grossAmount, serviceFee, mintAmount
            );
        }
    }

    function isDepositProcessed(bytes32 depositId) external view returns (bool) {
        return _processedDeposits[depositId];
    }

    function _validateAndMarkDeposit(IBridge.DepositMintRequest calldata request) private returns (uint256 mintAmount) {
        if (request.recipient == address(0)) {
            revert IBridge.ZeroAddress();
        }
        if (_processedDeposits[request.depositId]) {
            revert IBridge.DepositAlreadyProcessed(request.depositId);
        }
        if (serviceFee > request.maxServiceFee) {
            revert IBridge.ServiceFeeExceedsUserMaximum(serviceFee, request.maxServiceFee);
        }

        if (request.grossAmount <= serviceFee) {
            revert IBridge.InvalidAmount(request.grossAmount);
        }
        mintAmount = MintAccounting.netAmount(request.grossAmount, serviceFee);
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
