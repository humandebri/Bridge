// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract MintCommitRollbackOmitted {
    mapping(bytes32 => bool) private processed;

    function commitAndSwallowFailure(bytes32 depositId, address target) external {
        processed[depositId] = true;
        (bool succeeded,) = target.call("");
        assert(!succeeded);
    }

    function isProcessed(bytes32 depositId) external view returns (bool) {
        return processed[depositId];
    }
}

contract AlwaysReverts {
    fallback() external payable {
        revert();
    }
}

contract MintCommitRollbackOmittedHalmos {
    function check_external_failure_must_rollback(bytes32 depositId) public {
        MintCommitRollbackOmitted mutant = new MintCommitRollbackOmitted();
        AlwaysReverts target = new AlwaysReverts();
        mutant.commitAndSwallowFailure(depositId, address(target));
        assert(!mutant.isProcessed(depositId));
    }
}
