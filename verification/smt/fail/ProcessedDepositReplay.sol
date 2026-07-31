pragma solidity 0.8.36;
contract ProcessedDepositReplay {
    function acceptsProcessedDeposit() external pure {
        bool processed = true;
        bool accepted = processed;
        assert(!accepted);
    }
}
