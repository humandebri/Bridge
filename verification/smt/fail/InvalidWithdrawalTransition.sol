// verification/smt/fail: prove the gate rejects a model that permits Released to become Refunded.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBridge} from "bridge-src/interfaces/IBridge.sol";

contract InvalidWithdrawalTransition {
    function refundAfterRelease(bool performRefund) external pure returns (IBridge.WithdrawalStatus status) {
        status = IBridge.WithdrawalStatus.Released;
        if (performRefund) {
            status = IBridge.WithdrawalStatus.Refunded;
        }
        assert(status != IBridge.WithdrawalStatus.Refunded);
    }
}
