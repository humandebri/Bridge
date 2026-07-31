// verification/smt/pass: prove scalar kernels called by the production Mint transition.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {MintAuthorizationPolicy} from "bridge-src/libraries/MintAuthorizationPolicy.sol";
import {MintAccounting} from "bridge-src/libraries/MintAccounting.sol";

contract MintAuthorizationState {
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

    function acceptedAmounts(
        uint256 grossAmount,
        uint256 chargedFee,
        uint256 userMaximum,
        uint256 protocolMaximum,
        uint256 perDepositLimit
    ) external pure {
        bool fee = MintAuthorizationPolicy.feeWithinBounds(chargedFee, userMaximum, protocolMaximum);
        (bool amount, uint256 minted) =
            MintAuthorizationPolicy.mintAmountWithinLimit(grossAmount, chargedFee, perDepositLimit);
        if (
            grossAmount <= type(uint128).max && chargedFee <= type(uint128).max
                && userMaximum <= type(uint128).max && fee && amount
        ) {
            assert(chargedFee <= userMaximum);
            assert(chargedFee <= protocolMaximum);
            assert(minted > 0);
            assert(minted == grossAmount - chargedFee);
            assert(minted <= perDepositLimit);
        }
    }

    function epochStrictlyIncreases(uint256 currentEpoch, address currentSigner, address nextSigner)
        external
        pure
    {
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

    function processedDepositCannotBeAccepted(bool processed) external pure {
        if (processed) {
            assert(!MintAuthorizationPolicy.depositAvailable(processed));
        }
    }

    function successfulMintUsesOneAmount(
        uint256 grossAmount,
        uint256 chargedFee,
        uint256 consumed,
        uint256 windowLimit
    ) external pure {
        require(grossAmount > chargedFee);
        uint256 mintAmount;
        uint256 supplyIncrease;
        uint256 eventGrossAmount;
        uint256 eventServiceFee;
        uint256 eventMintedAmount;
        (mintAmount, supplyIncrease, eventGrossAmount, eventServiceFee, eventMintedAmount) =
            MintAuthorizationPolicy.mintEffectAmounts(grossAmount, chargedFee);
        (bool accepted, uint256 consumedAfter,) =
            MintAccounting.tryConsumeWindow(consumed, mintAmount, windowLimit);
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
}
