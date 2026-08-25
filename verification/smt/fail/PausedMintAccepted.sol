pragma solidity 0.8.36;
contract PausedMintAccepted {
    function acceptsPausedMint() external pure {
        bool paused = true;
        bool accepted = true;
        assert(!paused || !accepted);
    }
}
