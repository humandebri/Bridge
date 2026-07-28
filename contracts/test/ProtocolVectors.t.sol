// contracts/test: bind the production Withdrawal implementation to Lean-generated protocol vectors.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBSNS} from "../src/interfaces/IBSNS.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";
import {TestBase} from "./TestBase.sol";

contract ProtocolVectorsTest is TestBase {
    string private constant VECTORS = "../verification/generated/protocol-vectors.json";
    string private constant MINT_AUTHORIZATION_VECTOR = "../verification/generated/mint-authorization-vector.json";
    uint256 private constant BRIDGE_SIGNER_KEY = 0xA11CE;
    address private constant RUNTIME_ADMINISTRATOR = address(0x22);
    address private constant USER = address(0x44);

    function _deployBridge(uint256 serviceFee) private returns (Bridge bridge, IBSNS token) {
        address bridgeSigner = vm.addr(BRIDGE_SIGNER_KEY);
        address timelock = _deployTestTimelock(address(0x33));
        bridge = new Bridge(
            "kinic",
            "KINIC",
            8,
            bridgeSigner,
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
        IBridge.MintAuthorization memory authorization = IBridge.MintAuthorization({
            depositId: keccak256(abi.encode(serviceFee)),
            recipient: USER,
            grossAmount: 1_100,
            maxServiceFee: serviceFee,
            chargedServiceFee: serviceFee,
            deadline: block.timestamp + 30 minutes,
            authorizationEpoch: bridge.mintAuthorizationEpoch()
        });
        _submitMintAuthorization(BRIDGE_SIGNER_KEY, bridge, authorization, address(this));
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

    function test_mint_authorization_shared_vector() public view {
        string memory json = vm.readFile(MINT_AUTHORIZATION_VECTOR);
        bytes32 depositId = _bytes32(_hexBytes(vm.parseJsonString(json, ".authorization.deposit_id")));
        address recipient = _address(_hexBytes(vm.parseJsonString(json, ".authorization.recipient")));
        address verifyingContract = _address(_hexBytes(vm.parseJsonString(json, ".domain.verifying_contract")));
        uint256 chainId = vm.parseUint(vm.parseJsonString(json, ".domain.chain_id"));
        bytes32 domainSeparator = keccak256(
            abi.encode(
                keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)"),
                keccak256(bytes(vm.parseJsonString(json, ".domain.name"))),
                keccak256(bytes(vm.parseJsonString(json, ".domain.version"))),
                chainId,
                verifyingContract
            )
        );
        bytes32 structHash = keccak256(
            abi.encode(
                keccak256(
                    "MintAuthorization(bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 authorizationEpoch)"
                ),
                depositId,
                recipient,
                vm.parseUint(vm.parseJsonString(json, ".authorization.gross_amount")),
                vm.parseUint(vm.parseJsonString(json, ".authorization.max_service_fee")),
                vm.parseUint(vm.parseJsonString(json, ".authorization.charged_service_fee")),
                vm.parseUint(vm.parseJsonString(json, ".authorization.deadline")),
                vm.parseUint(vm.parseJsonString(json, ".authorization.authorization_epoch"))
            )
        );
        bytes32 digest = keccak256(abi.encodePacked(hex"1901", domainSeparator, structHash));
        assert(digest == _bytes32(_hexBytes(vm.parseJsonString(json, ".digest"))));

        bytes memory signature = _hexBytes(vm.parseJsonString(json, ".signature"));
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := mload(add(signature, 0x20))
            s := mload(add(signature, 0x40))
            v := byte(0, mload(add(signature, 0x60)))
        }
        assert(ecrecover(digest, v, r, s) == _address(_hexBytes(vm.parseJsonString(json, ".signer"))));
    }

    function _bytes32(bytes memory value) private pure returns (bytes32 result) {
        assert(value.length == 32);
        assembly {
            result := mload(add(value, 0x20))
        }
    }

    function _address(bytes memory value) private pure returns (address result) {
        assert(value.length == 20);
        assembly {
            result := shr(96, mload(add(value, 0x20)))
        }
    }

    function _hexBytes(string memory encoded) private pure returns (bytes memory result) {
        bytes memory value = bytes(encoded);
        assert(value.length >= 2 && value[0] == "0" && value[1] == "x");
        assert((value.length - 2) % 2 == 0);
        result = new bytes((value.length - 2) / 2);
        for (uint256 i = 0; i < result.length; ++i) {
            result[i] = bytes1((_nibble(value[2 + i * 2]) << 4) | _nibble(value[3 + i * 2]));
        }
    }

    function _nibble(bytes1 value) private pure returns (uint8) {
        uint8 digit = uint8(value);
        if (digit >= 48 && digit <= 57) {
            return digit - 48;
        }
        if (digit >= 97 && digit <= 102) {
            return digit - 97 + 10;
        }
        revert();
    }
}
