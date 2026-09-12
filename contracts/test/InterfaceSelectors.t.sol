// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {IBridge} from "../src/interfaces/IBridge.sol";

contract InterfaceSelectorsTest {
    // ABI snapshots encode this field as uint8 and cannot detect ordinal changes.
    function testWithdrawalStatusOrdinals() public pure {
        assert(uint8(IBridge.WithdrawalStatus.None) == 0);
        assert(uint8(IBridge.WithdrawalStatus.Committed) == 1);
    }
}
