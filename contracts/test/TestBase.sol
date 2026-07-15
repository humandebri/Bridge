// contracts/test: provide only the Foundry cheatcodes needed by Base contract phases without another dependency.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

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
}

abstract contract TestBase {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

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
            abi.encode(72 hours, proposers, cancellers, executors)
        );
    }

    function _timelockCodeHash(address timelock) internal view returns (bytes32) {
        return timelock.codehash;
    }
}
