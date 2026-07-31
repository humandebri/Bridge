pragma solidity 0.8.36;
contract MintLimitOmitted {
    function acceptsOverLimit(uint256 limit) external pure {
        require(limit < type(uint256).max);
        uint256 minted = limit + 1;
        assert(minted <= limit);
    }
}
