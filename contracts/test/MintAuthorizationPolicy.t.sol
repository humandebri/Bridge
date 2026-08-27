// contracts/test: exercise every production Mint transition result and its effects.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {MintAuthorizationPolicy} from "../src/libraries/MintAuthorizationPolicy.sol";

contract MintAuthorizationPolicyTest {
    function _valid() private pure returns (MintAuthorizationPolicy.MintTransitionInput memory input) {
        input = MintAuthorizationPolicy.MintTransitionInput({
            timestamp: 0,
            deadline: 0,
            authorizationEpoch: 1,
            currentEpoch: 1,
            recipient: address(1),
            bridge: address(2),
            token: address(3),
            grossAmount: 2,
            maximumFee: 1,
            chargedFee: 1,
            protocolMaximumFee: 1,
            perDepositLimit: 1,
            consumedInWindow: 0,
            windowLimit: 1,
            windowStartedAt: 0,
            windowDuration: 1,
            paused: false,
            processed: false
        });
    }

    function _assertRejected(
        MintAuthorizationPolicy.MintTransitionInput memory input,
        MintAuthorizationPolicy.RejectReason expected
    ) private pure {
        (MintAuthorizationPolicy.RejectReason reason, MintAuthorizationPolicy.MintEffects memory effects,) =
            MintAuthorizationPolicy.evaluateMint(input);
        assert(reason == expected);
        assert(!effects.processedAfter);
        assert(!effects.windowReset);
        assert(effects.windowStartedAtAfter == 0);
        assert(effects.windowConsumedAfter == 0);
        assert(effects.mintAmount == 0);
        assert(effects.supplyIncrease == 0);
        assert(effects.eventGrossAmount == 0);
        assert(effects.eventServiceFee == 0);
        assert(effects.eventMintedAmount == 0);
    }

    function testEveryRejectionHasZeroEffects() public pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _valid();
        input.paused = true;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.Paused);

        input = _valid();
        input.timestamp = 2;
        input.deadline = 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.Expired);

        input = _valid();
        input.deadline = 15 minutes + 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.DeadlineTooFar);

        input = _valid();
        input.authorizationEpoch = 2;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.EpochMismatch);

        input = _valid();
        input.recipient = address(0);
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.ZeroRecipient);

        input = _valid();
        input.recipient = input.bridge;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.InvalidRecipient);

        input = _valid();
        input.recipient = input.token;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.InvalidRecipient);

        input = _valid();
        input.grossAmount = uint256(type(uint128).max) + 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.GrossExceedsU128);

        input = _valid();
        input.maximumFee = uint256(type(uint128).max) + 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.MaximumFeeExceedsU128);

        input = _valid();
        input.chargedFee = uint256(type(uint128).max) + 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.ChargedFeeExceedsU128);

        input = _valid();
        input.processed = true;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.Processed);

        input = _valid();
        input.grossAmount = 3;
        input.chargedFee = 2;
        input.maximumFee = 2;
        input.protocolMaximumFee = 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.ProtocolFeeExceeded);

        input = _valid();
        input.grossAmount = 3;
        input.chargedFee = 2;
        input.maximumFee = 1;
        input.protocolMaximumFee = 2;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.UserFeeExceeded);

        input = _valid();
        input.grossAmount = 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.InvalidAmount);

        input = _valid();
        input.grossAmount = 3;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.PerDepositLimitExceeded);

        input = _valid();
        input.grossAmount = 3;
        input.perDepositLimit = 2;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.WindowLimitExceeded);

        input = _valid();
        input.timestamp = uint256(type(uint64).max) + 1;
        input.deadline = input.timestamp + 15 minutes;
        input.windowDuration = 0;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.TimestampExceedsU64);
    }

    function testAcceptedEffectsUseOneAmountAndExactWindowState() public pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _valid();
        input.timestamp = 10;
        input.deadline = 10;
        input.grossAmount = 11;
        input.chargedFee = 1;
        input.maximumFee = 1;
        input.protocolMaximumFee = 1;
        input.perDepositLimit = 10;
        input.windowLimit = 10;
        input.windowStartedAt = 9;
        input.windowDuration = 1;
        (MintAuthorizationPolicy.RejectReason reason, MintAuthorizationPolicy.MintEffects memory effects,) =
            MintAuthorizationPolicy.evaluateMint(input);
        assert(reason == MintAuthorizationPolicy.RejectReason.None);
        assert(effects.processedAfter);
        assert(effects.windowReset);
        assert(effects.windowStartedAtAfter == 10);
        assert(effects.windowConsumedAfter == 10);
        assert(effects.mintAmount == 10);
        assert(effects.supplyIncrease == 10);
        assert(effects.eventGrossAmount == 11);
        assert(effects.eventServiceFee == 1);
        assert(effects.eventMintedAmount == 10);
    }

    function testDeadlineAcceptsFifteenMinutesAndRejectsOneSecondMore() public pure {
        MintAuthorizationPolicy.MintTransitionInput memory input = _valid();
        input.timestamp = 1_000;
        input.deadline = input.timestamp + 15 minutes;
        (MintAuthorizationPolicy.RejectReason accepted,,) = MintAuthorizationPolicy.evaluateMint(input);
        assert(accepted == MintAuthorizationPolicy.RejectReason.None);

        input.deadline += 1;
        _assertRejected(input, MintAuthorizationPolicy.RejectReason.DeadlineTooFar);
    }
}
