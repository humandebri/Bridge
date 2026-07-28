// contracts/test: provide only the Foundry cheatcodes needed by Base contract phases without another dependency.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {Bridge} from "../src/Bridge.sol";
import {IBridge} from "../src/interfaces/IBridge.sol";

interface Vm {
    struct Log {
        bytes32[] topics;
        bytes data;
        address emitter;
    }

    function addr(uint256 privateKey) external returns (address);
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
    function prank(address caller) external;
    function startPrank(address caller) external;
    function stopPrank() external;
    function warp(uint256 timestamp) external;
    function chainId(uint256 newChainId) external;
    function expectRevert(bytes4 revertData) external;
    function expectRevert(bytes calldata revertData) external;
    function expectPartialRevert(bytes4 revertData) external;
    function expectEmit(bool checkTopic1, bool checkTopic2, bool checkTopic3, bool checkData, address emitter) external;
    function recordLogs() external;
    function getRecordedLogs() external returns (Log[] memory logs);
    function bound(uint256 value, uint256 minimum, uint256 maximum) external pure returns (uint256 result);
    function deployCode(string calldata artifactPath, bytes calldata constructorArgs)
        external
        returns (address deployed);
    function readFile(string calldata path) external view returns (string memory data);
    function parseJsonUint(string calldata json, string calldata key) external pure returns (uint256 value);
    function parseJsonBool(string calldata json, string calldata key) external pure returns (bool value);
    function parseJsonString(string calldata json, string calldata key) external pure returns (string memory value);
    function parseUint(string calldata value) external pure returns (uint256 parsed);
    function toString(uint256 value) external pure returns (string memory text);
}

abstract contract TestBase {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));
    bytes32 internal constant MINT_AUTHORIZATION_TYPEHASH = keccak256(
        "MintAuthorization(bytes32 depositId,address recipient,uint256 grossAmount,uint256 maxServiceFee,uint256 chargedServiceFee,uint256 deadline,uint256 authorizationEpoch)"
    );
    bytes32 internal constant MINT_EIP712_DOMAIN_TYPEHASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");

    function _sameString(string memory left, string memory right) internal pure returns (bool) {
        return keccak256(bytes(left)) == keccak256(bytes(right));
    }

    function _deployTestTimelock(address operator) internal returns (address) {
        address[] memory proposers = new address[](1);
        proposers[0] = operator;
        address[] memory cancellers = new address[](1);
        cancellers[0] = address(uint160(operator) ^ uint160(0xCA11));
        address[] memory executors = new address[](1);
        executors[0] = operator;
        return vm.deployCode(
            "BridgeTimelockController.sol:BridgeTimelockController",
            abi.encode(24 hours, proposers, cancellers, executors)
        );
    }

    function _timelockCodeHash(address timelock) internal view returns (bytes32) {
        return timelock.codehash;
    }

    function _mintAuthorizationDigest(address bridge, IBridge.MintAuthorization memory authorization)
        internal
        view
        returns (bytes32)
    {
        bytes32 domainSeparator = keccak256(
            abi.encode(MINT_EIP712_DOMAIN_TYPEHASH, keccak256("KINIC Bridge"), keccak256("1"), block.chainid, bridge)
        );
        bytes32 structHash = keccak256(
            abi.encode(
                MINT_AUTHORIZATION_TYPEHASH,
                authorization.depositId,
                authorization.recipient,
                authorization.grossAmount,
                authorization.maxServiceFee,
                authorization.chargedServiceFee,
                authorization.deadline,
                authorization.authorizationEpoch
            )
        );
        return keccak256(abi.encodePacked(hex"1901", domainSeparator, structHash));
    }

    function _signMintAuthorization(uint256 signerKey, Bridge bridge, IBridge.MintAuthorization memory authorization)
        internal
        returns (bytes memory)
    {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(signerKey, _mintAuthorizationDigest(address(bridge), authorization));
        return abi.encodePacked(r, s, v);
    }

    function _submitMintAuthorization(
        uint256 signerKey,
        Bridge bridge,
        IBridge.MintAuthorization memory authorization,
        address caller
    ) internal {
        bytes memory signature = _signMintAuthorization(signerKey, bridge, authorization);
        vm.prank(caller);
        bridge.mintDepositWithAuthorization(authorization, signature);
    }
}
