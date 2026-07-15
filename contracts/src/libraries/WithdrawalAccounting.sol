// contracts/src/libraries: share Withdrawal settlement decisions with Bridge and SMT fixtures.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBridge} from "../interfaces/IBridge.sol";

library WithdrawalAccounting {
    enum ReleaseAction {
        Reject,
        Apply,
        Idempotent
    }

    function releaseAction(IBridge.WithdrawalStatus status, bool detailsMatch) internal pure returns (ReleaseAction) {
        if (status == IBridge.WithdrawalStatus.Releasing) {
            return ReleaseAction.Apply;
        }
        if (status == IBridge.WithdrawalStatus.Released && detailsMatch) {
            return ReleaseAction.Idempotent;
        }
        return ReleaseAction.Reject;
    }

    function refundAllowed(IBridge.WithdrawalStatus status) internal pure returns (bool) {
        return status == IBridge.WithdrawalStatus.Pending;
    }

    function cancelAllowed(IBridge.WithdrawalStatus status) internal pure returns (bool) {
        return status == IBridge.WithdrawalStatus.Releasing;
    }

    function tryRecordLedgerBlock(uint256 existingWithdrawalId, uint256 withdrawalId)
        internal
        pure
        returns (bool accepted, uint256 recordedWithdrawalId)
    {
        if (existingWithdrawalId != 0) {
            return (false, existingWithdrawalId);
        }
        return (true, withdrawalId);
    }

    function feeWithinMaximum(uint256 withdrawalServiceFee, uint256 maximumServiceFee) internal pure returns (bool) {
        return withdrawalServiceFee <= maximumServiceFee;
    }

    function meetsMinimum(uint256 amountOut, uint256 minAmountOut) internal pure returns (bool) {
        return amountOut >= minAmountOut;
    }

    function settlementMatches(uint256 amount, uint256 amountOut, uint256 withdrawalServiceFee, uint256 ledgerFee)
        internal
        pure
        returns (bool)
    {
        if (amountOut > amount) {
            return false;
        }
        uint256 afterAmountOut = amount - amountOut;
        if (withdrawalServiceFee > afterAmountOut) {
            return false;
        }
        return ledgerFee == afterAmountOut - withdrawalServiceFee;
    }
}
