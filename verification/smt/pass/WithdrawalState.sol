// verification/smt/pass: prove production Withdrawal settlement decisions and terminal-state exclusions.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBridge} from "bridge-src/interfaces/IBridge.sol";
import {WithdrawalAccounting} from "bridge-src/libraries/WithdrawalAccounting.sol";

contract WithdrawalState {
    function settlement(uint256 amount, uint256 amountOut, uint256 serviceFee, uint256 ledgerFee) external pure {
        bool matches = WithdrawalAccounting.settlementMatches(amount, amountOut, serviceFee, ledgerFee);
        if (matches) {
            assert(amountOut <= amount);
            uint256 afterAmountOut = amount - amountOut;
            assert(serviceFee <= afterAmountOut);
            assert(ledgerFee == afterAmountOut - serviceFee);
        }
    }

    function releaseDecision(IBridge.WithdrawalStatus status, bool detailsMatch) external pure {
        WithdrawalAccounting.ReleaseAction action = WithdrawalAccounting.releaseAction(status, detailsMatch);
        if (action == WithdrawalAccounting.ReleaseAction.Apply) {
            assert(status == IBridge.WithdrawalStatus.Pending);
        } else if (action == WithdrawalAccounting.ReleaseAction.Idempotent) {
            assert(status == IBridge.WithdrawalStatus.Released);
            assert(detailsMatch);
        } else {
            assert(status != IBridge.WithdrawalStatus.Pending);
            assert(status != IBridge.WithdrawalStatus.Released || !detailsMatch);
        }
    }

    function refundDecision(IBridge.WithdrawalStatus status) external pure {
        bool allowed = WithdrawalAccounting.refundAllowed(status);
        if (allowed) {
            assert(status == IBridge.WithdrawalStatus.Pending);
        } else {
            assert(status != IBridge.WithdrawalStatus.Pending);
        }
    }

    function terminalStatesAreExclusive(bool releasedDetailsMatch) external pure {
        assert(!WithdrawalAccounting.refundAllowed(IBridge.WithdrawalStatus.Released));
        assert(
            WithdrawalAccounting.releaseAction(IBridge.WithdrawalStatus.Refunded, releasedDetailsMatch)
                == WithdrawalAccounting.ReleaseAction.Reject
        );
    }

    function feeAndMinimum(uint256 serviceFee, uint256 maximumServiceFee, uint256 amountOut, uint256 minAmountOut)
        external
        pure
    {
        if (WithdrawalAccounting.feeWithinMaximum(serviceFee, maximumServiceFee)) {
            assert(serviceFee <= maximumServiceFee);
        }
        if (WithdrawalAccounting.meetsMinimum(amountOut, minAmountOut)) {
            assert(amountOut >= minAmountOut);
        }
    }
}
