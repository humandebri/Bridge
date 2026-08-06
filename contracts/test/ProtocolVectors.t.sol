// contracts/test: non-generated known-answer checks for the shared mint authorization vector.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {TestBase} from "./TestBase.sol";

contract ProtocolVectorsTest is TestBase {
    string private constant MINT_AUTHORIZATION_VECTOR = "../verification/generated/mint-authorization-vector.json";

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
        if (digit >= 48 && digit <= 57) return digit - 48;
        if (digit >= 97 && digit <= 102) return digit - 97 + 10;
        revert();
    }
}
