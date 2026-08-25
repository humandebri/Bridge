// contracts/src/libraries: production-shared Mint Authorization safety predicates.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {MintAccounting} from "./MintAccounting.sol";

library MintAuthorizationPolicy {
    enum RejectReason {
        None,
        Paused,
        Expired,
        DeadlineTooFar,
        EpochMismatch,
        ZeroRecipient,
        InvalidRecipient,
        GrossExceedsU128,
        MaximumFeeExceedsU128,
        ChargedFeeExceedsU128,
        Processed,
        ProtocolFeeExceeded,
        UserFeeExceeded,
        InvalidAmount,
        PerDepositLimitExceeded,
        WindowLimitExceeded,
        TimestampExceedsU64
    }

    struct MintTransitionInput {
        uint256 timestamp;
        uint256 deadline;
        uint256 authorizationEpoch;
        uint256 currentEpoch;
        address recipient;
        address bridge;
        address token;
        uint256 grossAmount;
        uint256 maximumFee;
        uint256 chargedFee;
        uint256 protocolMaximumFee;
        uint256 perDepositLimit;
        uint256 consumedInWindow;
        uint256 windowLimit;
        uint64 windowStartedAt;
        uint64 windowDuration;
        bool paused;
        bool processed;
    }

    struct MintEffects {
        bool processedAfter;
        bool windowReset;
        uint64 windowStartedAtAfter;
        uint256 windowConsumedAfter;
        uint256 mintAmount;
        uint256 supplyIncrease;
        uint256 eventGrossAmount;
        uint256 eventServiceFee;
        uint256 eventMintedAmount;
    }

    function evaluateMint(MintTransitionInput memory input)
        internal
        pure
        returns (RejectReason reason, MintEffects memory effects, uint256 windowAvailable)
    {
        if (input.paused) return (RejectReason.Paused, effects, 0);
        if (!deadlineAccepts(input.timestamp, input.deadline)) return (RejectReason.Expired, effects, 0);
        if (input.deadline > input.timestamp + 15 minutes) return (RejectReason.DeadlineTooFar, effects, 0);
        if (!epochMatches(input.authorizationEpoch, input.currentEpoch)) {
            return (RejectReason.EpochMismatch, effects, 0);
        }
        if (input.recipient == address(0)) return (RejectReason.ZeroRecipient, effects, 0);
        if (input.recipient == input.bridge || input.recipient == input.token) {
            return (RejectReason.InvalidRecipient, effects, 0);
        }
        if (input.grossAmount > type(uint128).max) return (RejectReason.GrossExceedsU128, effects, 0);
        if (input.maximumFee > type(uint128).max) return (RejectReason.MaximumFeeExceedsU128, effects, 0);
        if (input.chargedFee > type(uint128).max) return (RejectReason.ChargedFeeExceedsU128, effects, 0);
        if (!depositAvailable(input.processed)) return (RejectReason.Processed, effects, 0);
        if (!feeWithinBounds(input.chargedFee, input.maximumFee, input.protocolMaximumFee)) {
            if (input.chargedFee > input.protocolMaximumFee) {
                return (RejectReason.ProtocolFeeExceeded, effects, 0);
            }
            return (RejectReason.UserFeeExceeded, effects, 0);
        }
        (bool amountAccepted, uint256 mintAmount) =
            mintAmountWithinLimit(input.grossAmount, input.chargedFee, input.perDepositLimit);
        if (input.grossAmount <= input.chargedFee) {
            return (RejectReason.InvalidAmount, effects, 0);
        }
        if (!amountAccepted) {
            return (RejectReason.PerDepositLimitExceeded, effects, 0);
        }

        bool reset = windowExpired(input.timestamp, input.windowStartedAt, input.windowDuration);
        if (reset && input.timestamp > type(uint64).max) {
            return (RejectReason.TimestampExceedsU64, effects, 0);
        }
        uint256 consumed = reset ? 0 : input.consumedInWindow;
        (bool accepted, uint256 nextConsumed, uint256 available) =
            MintAccounting.tryConsumeWindow(consumed, mintAmount, input.windowLimit);
        if (!accepted) {
            return (RejectReason.WindowLimitExceeded, effects, available);
        }

        effects.processedAfter = true;
        effects.windowReset = reset;
        effects.windowStartedAtAfter = reset ? uint64(input.timestamp) : input.windowStartedAt;
        effects.windowConsumedAfter = nextConsumed;
        (
            effects.mintAmount,
            effects.supplyIncrease,
            effects.eventGrossAmount,
            effects.eventServiceFee,
            effects.eventMintedAmount
        ) = mintEffectAmounts(input.grossAmount, input.chargedFee);
        return (RejectReason.None, effects, available);
    }

    function feeWithinBounds(uint256 chargedFee, uint256 userMaximum, uint256 protocolMaximum)
        internal
        pure
        returns (bool)
    {
        return chargedFee <= userMaximum && chargedFee <= protocolMaximum;
    }

    function mintAmountWithinLimit(uint256 grossAmount, uint256 chargedFee, uint256 perDepositLimit)
        internal
        pure
        returns (bool accepted, uint256 mintAmount)
    {
        if (grossAmount <= chargedFee) return (false, 0);
        mintAmount = grossAmount - chargedFee;
        return (mintAmount <= perDepositLimit, mintAmount);
    }

    function deadlineAccepts(uint256 timestamp, uint256 deadline) internal pure returns (bool) {
        return timestamp <= deadline;
    }

    function epochMatches(uint256 authorizationEpoch, uint256 currentEpoch) internal pure returns (bool) {
        return authorizationEpoch == currentEpoch;
    }

    function depositAvailable(bool processed) internal pure returns (bool) {
        return !processed;
    }

    function mintEffectAmounts(uint256 grossAmount, uint256 chargedFee)
        internal
        pure
        returns (
            uint256 mintAmount,
            uint256 supplyIncrease,
            uint256 eventGrossAmount,
            uint256 eventServiceFee,
            uint256 eventMintedAmount
        )
    {
        mintAmount = grossAmount - chargedFee;
        return (mintAmount, mintAmount, grossAmount, chargedFee, mintAmount);
    }

    function nextEpoch(uint256 currentEpoch) internal pure returns (uint256) {
        return currentEpoch + 1;
    }

    function signerRotationEpoch(uint256 currentEpoch, address currentSigner, address nextSigner)
        internal
        pure
        returns (bool changed, uint256 resultingEpoch)
    {
        return adminEpochTransition(currentEpoch, currentSigner != nextSigner);
    }

    function adminEpochTransition(uint256 currentEpoch, bool changed)
        internal
        pure
        returns (bool applied, uint256 resultingEpoch)
    {
        if (!changed) return (false, currentEpoch);
        return (true, nextEpoch(currentEpoch));
    }

    function windowExpired(uint256 timestamp, uint64 startedAt, uint64 duration) internal pure returns (bool) {
        return timestamp >= uint256(startedAt) + uint256(duration);
    }
}
