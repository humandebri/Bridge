pragma solidity 0.8.36;
contract NetAmountConstraintOmitted {
    function acceptsUnrelatedAmount(uint256 gross, uint256 fee, uint256 minted) external pure {
        require(gross > fee && minted != gross - fee);
        assert(minted == gross - fee);
    }
}
