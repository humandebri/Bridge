// contracts/test: bind the production Withdrawal implementation to Lean-generated protocol vectors.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";

contract ProtocolVectorsTest is TestBase {
    string private constant VECTORS = "../verification/generated/protocol-vectors.json";
    address private constant BRIDGE_SIGNER = address(0x11);
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant USER = address(0x44);

    function _deployBridge(uint256 serviceFee) private returns (Bridge bridge, IBSNS token) {
        address timelock = _deployTestTimelock(address(0x33));
        bridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            BRIDGE_SIGNER,
            RUNTIME_ADMINISTRATOR,
            timelock,
            _timelockCodeHash(timelock),
            2_000,
            2_000,
            1 hours,
            100,
            serviceFee
        );
        token = bridge.bsns();
        vm.prank(timelock);
        bridge.unpauseDepositMints();
        vm.prank(timelock);
        bridge.unpauseWithdrawals();
        vm.prank(BRIDGE_SIGNER);
        bridge.mintDeposit(
            IBridge.DepositMintRequest(keccak256(abi.encode(serviceFee)), USER, 1_100, serviceFee, serviceFee)
        );
    }

    function test_protocol_quote_cases_matches_production() public {
        string memory json = vm.readFile(VECTORS);
        assert(vm.parseJsonUint(json, ".schema_version") == 2);
        uint256 count = vm.parseJsonUint(json, ".quote_count");
        assert(count > 0);

        for (uint256 index = 0; index < count; ++index) {
            string memory base = string.concat(".quote_cases[", vm.toString(index), "]");
            uint256 amount = vm.parseUint(vm.parseJsonString(json, string.concat(base, ".amount")));
            uint256 serviceFee = vm.parseUint(vm.parseJsonString(json, string.concat(base, ".service_fee")));
            bool accepted = vm.parseJsonBool(json, string.concat(base, ".accepted"));
            (Bridge bridge, IBSNS token) = _deployBridge(serviceFee);

            vm.prank(USER);
            token.approve(address(bridge), amount);

            if (!accepted) {
                vm.prank(USER);
                vm.expectRevert(abi.encodeWithSelector(IBridge.InvalidAmount.selector, amount));
                bridge.createWithdrawal(amount, serviceFee, hex"01", bytes32(0));
                continue;
            }

            uint256 expectedAmountOut = vm.parseUint(vm.parseJsonString(json, string.concat(base, ".amount_out")));
            vm.prank(USER);
            uint256 withdrawalId = bridge.createWithdrawal(amount, serviceFee, hex"01", bytes32(0));
            IBridge.Withdrawal memory withdrawal = bridge.getWithdrawal(withdrawalId);
            assert(withdrawal.amount == amount);
            assert(withdrawal.chargedServiceFee == serviceFee);
            assert(withdrawal.amountOut == expectedAmountOut);
            assert(withdrawal.status == IBridge.WithdrawalStatus.Committed);
        }
    }
}
