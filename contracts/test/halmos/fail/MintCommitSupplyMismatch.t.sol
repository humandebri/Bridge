// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

contract MintCommitSupplyMismatch {
    uint256 public supply;

    function commit(uint128 amount) external {
        supply += uint256(amount) + 1;
    }
}

contract MintCommitSupplyMismatchHalmos {
    function check_supply_amount_must_match(uint128 amount) public {
        MintCommitSupplyMismatch mutant = new MintCommitSupplyMismatch();
        uint256 beforeSupply = mutant.supply();
        mutant.commit(amount);
        assert(mutant.supply() == beforeSupply + amount);
    }
}
