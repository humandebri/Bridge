pragma solidity 0.8.36;
contract MissingEpochGuard {
    function acceptsOldEpoch(uint256 oldEpoch, uint256 currentEpoch) external pure {
        require(oldEpoch != currentEpoch);
        bool accepted = true;
        assert(!accepted);
    }
}
