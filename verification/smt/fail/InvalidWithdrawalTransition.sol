// verification/smt/fail: an inconsistent quote must not satisfy the committed invariant.
pragma solidity 0.8.36;
contract InvalidWithdrawalTransition {
    function invalidQuote() external pure {
        uint256 amount = 100;
        uint256 serviceFee = 10;
        uint256 amountOut = 91;
        assert(amountOut + serviceFee == amount);
    }
}
