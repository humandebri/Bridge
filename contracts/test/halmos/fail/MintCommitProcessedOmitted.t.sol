// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract MintCommitProcessedOmitted {
    mapping(bytes32 => bool) private processed;

    function commit(bytes32) external {}

    function isProcessed(bytes32 depositId) external view returns (bool) {
        return processed[depositId];
    }
}

contract MintCommitProcessedOmittedHalmos {
    function check_processed_update_is_required(bytes32 depositId) public {
        MintCommitProcessedOmitted mutant = new MintCommitProcessedOmitted();
        mutant.commit(depositId);
        assert(mutant.isProcessed(depositId));
    }
}
