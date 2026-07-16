// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract WithdrawalStateModel {
    enum Status { None, Committed }

    struct Withdrawal {
        uint256 amount;
        uint256 chargedServiceFee;
        uint256 amountOut;
        Status status;
    }

    function commit(uint256 amount, uint256 serviceFee) external pure returns (Withdrawal memory w) {
        require(serviceFee < amount);
        w = Withdrawal(amount, serviceFee, amount - serviceFee, Status.Committed);
        assert(w.status == Status.Committed);
        assert(w.amountOut + w.chargedServiceFee == w.amount);
    }

    function committedIsAbsorbing(Status status) external pure {
        if (status == Status.Committed) {
            Status next = status;
            assert(next == Status.Committed);
        }
    }
}
