// verification/smt/pass: prove scalar kernels called by the production Mint transition.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {MintAuthorizationPolicy} from "bridge-src/libraries/MintAuthorizationPolicy.sol";
import {MintAccounting} from "bridge-src/libraries/MintAccounting.sol";

contract MintAuthorizationState {
    function acceptedTransitionEnforcesEnvelope(
        bool paused,
        uint256 timestamp,
        uint256 deadline,
        uint256 authorizationEpoch,
        uint256 currentEpoch,
        bool processed
    ) external pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _validInput();
        input.paused = paused;
        input.timestamp = timestamp;
        input.deadline = deadline;
        input.authorizationEpoch = authorizationEpoch;
        input.currentEpoch = currentEpoch;
        input.processed = processed;
        (MintAuthorizationPolicy.RejectReason reason,,) = MintAuthorizationPolicy.evaluateMint(input);
        if (reason == MintAuthorizationPolicy.RejectReason.None) {
            assert(!input.paused);
            assert(input.timestamp <= input.deadline);
            assert(input.authorizationEpoch == input.currentEpoch);
            assert(!input.processed);
        }
    }

    function acceptedTransitionAppliesExactEffects(
        uint128 grossAmount,
        uint128 chargedFee,
        uint128 consumedInWindow,
        uint128 windowLimit
    ) external pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _validInput();
        input.grossAmount = grossAmount;
        input.maximumFee = chargedFee;
        input.chargedFee = chargedFee;
        input.protocolMaximumFee = chargedFee;
        input.perDepositLimit = grossAmount;
        input.consumedInWindow = consumedInWindow;
        input.windowLimit = windowLimit;
        (MintAuthorizationPolicy.RejectReason reason, MintAuthorizationPolicy.MintEffects memory effects,) =
            MintAuthorizationPolicy.evaluateMint(input);
        if (reason == MintAuthorizationPolicy.RejectReason.None) {
            assert(effects.processedAfter);
            assert(effects.mintAmount == input.grossAmount - input.chargedFee);
            assert(effects.supplyIncrease == effects.mintAmount);
            assert(effects.eventGrossAmount == input.grossAmount);
            assert(effects.eventServiceFee == input.chargedFee);
            assert(effects.eventMintedAmount == effects.mintAmount);
        }
    }

    function processedTransitionIsRejected() external pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _validInput();
        input.processed = true;
        (MintAuthorizationPolicy.RejectReason reason,,) = MintAuthorizationPolicy.evaluateMint(input);
        assert(reason == MintAuthorizationPolicy.RejectReason.Processed);
    }

    function expiredTransitionIsRejected() external pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _validInput();
        input.timestamp = input.deadline + 1;
        (MintAuthorizationPolicy.RejectReason reason,,) = MintAuthorizationPolicy.evaluateMint(input);
        assert(reason == MintAuthorizationPolicy.RejectReason.Expired);
    }

    function mismatchedEpochIsRejected() external pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _validInput();
        input.authorizationEpoch = input.currentEpoch + 1;
        (MintAuthorizationPolicy.RejectReason reason,,) = MintAuthorizationPolicy.evaluateMint(input);
        assert(reason == MintAuthorizationPolicy.RejectReason.EpochMismatch);
    }

    function acceptedEnvelope(
        uint256 timestamp,
        uint256 deadline,
        uint256 authorizationEpoch,
        uint256 currentEpoch,
        bool processed
    ) external pure {
        if (
            MintAuthorizationPolicy.deadlineAccepts(timestamp, deadline)
                && MintAuthorizationPolicy.epochMatches(authorizationEpoch, currentEpoch)
                && MintAuthorizationPolicy.depositAvailable(processed)
        ) {
            assert(timestamp <= deadline);
            assert(authorizationEpoch == currentEpoch);
            assert(!processed);
        }
    }

    function acceptedTransitionEnforcesU128Envelope(
        uint256 grossAmount,
        uint256 chargedFee,
        uint256 userMaximum,
        uint256 protocolMaximum,
        uint256 perDepositLimit
    ) external pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _validInput();
        input.grossAmount = grossAmount;
        input.maximumFee = userMaximum;
        input.chargedFee = chargedFee;
        input.protocolMaximumFee = protocolMaximum;
        input.perDepositLimit = perDepositLimit;
        (MintAuthorizationPolicy.RejectReason reason,,) = MintAuthorizationPolicy.evaluateMint(input);
        if (reason == MintAuthorizationPolicy.RejectReason.None) {
            assert(input.grossAmount <= type(uint128).max);
            assert(input.maximumFee <= type(uint128).max);
            assert(input.chargedFee <= type(uint128).max);
        }
    }

    function epochStrictlyIncreases(uint256 currentEpoch, address currentSigner, address nextSigner) external pure {
        require(currentEpoch < type(uint256).max);
        (bool changed, uint256 resultingEpoch) =
            MintAuthorizationPolicy.signerRotationEpoch(currentEpoch, currentSigner, nextSigner);
        assert(changed == (currentSigner != nextSigner));
        if (changed) {
            assert(resultingEpoch == currentEpoch + 1);
            assert(resultingEpoch > currentEpoch);
        } else {
            assert(resultingEpoch == currentEpoch);
        }
    }

    function adminEpochTransitionIsMonotone(uint256 currentEpoch, bool changed) external pure {
        require(currentEpoch < type(uint256).max);
        (bool applied, uint256 resultingEpoch) = MintAuthorizationPolicy.adminEpochTransition(currentEpoch, changed);
        assert(applied == changed);
        if (changed) {
            assert(resultingEpoch == currentEpoch + 1);
            assert(resultingEpoch > currentEpoch);
        } else {
            assert(resultingEpoch == currentEpoch);
        }
    }

    function processedDepositCannotBeAccepted(bool processed) external pure {
        if (processed) {
            assert(!MintAuthorizationPolicy.depositAvailable(processed));
        }
    }

    function successfulMintUsesOneAmount(uint256 grossAmount, uint256 chargedFee, uint256 consumed, uint256 windowLimit)
        external
        pure
    {
        require(grossAmount > chargedFee);
        uint256 mintAmount;
        uint256 supplyIncrease;
        uint256 eventGrossAmount;
        uint256 eventServiceFee;
        uint256 eventMintedAmount;
        (mintAmount, supplyIncrease, eventGrossAmount, eventServiceFee, eventMintedAmount) =
            MintAuthorizationPolicy.mintEffectAmounts(grossAmount, chargedFee);
        (bool accepted, uint256 consumedAfter,) = MintAccounting.tryConsumeWindow(consumed, mintAmount, windowLimit);
        if (accepted) {
            assert(consumedAfter == consumed + mintAmount);
            assert(consumedAfter <= windowLimit);
            assert(supplyIncrease == mintAmount);
            assert(eventGrossAmount == grossAmount);
            assert(eventServiceFee == chargedFee);
            assert(eventMintedAmount == mintAmount);
        }
    }

    function windowResetsOnlyAtBoundary(uint256 timestamp, uint64 startedAt, uint64 duration) external pure {
        bool expired = MintAuthorizationPolicy.windowExpired(timestamp, startedAt, duration);
        if (expired) {
            assert(timestamp >= uint256(startedAt) + uint256(duration));
        } else {
            assert(timestamp < uint256(startedAt) + uint256(duration));
        }
    }

    function _validInput() private pure returns (MintAuthorizationPolicy.MintTransitionInput memory input) {
        input.timestamp = 1;
        input.deadline = 1;
        input.authorizationEpoch = 1;
        input.currentEpoch = 1;
        input.recipient = address(1);
        input.bridge = address(2);
        input.token = address(3);
        input.grossAmount = 2;
        input.maximumFee = 1;
        input.chargedFee = 1;
        input.protocolMaximumFee = 1;
        input.perDepositLimit = 1;
        input.windowLimit = 1;
        input.windowDuration = 1;
    }
}
