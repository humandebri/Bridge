// contracts/test: provide only the Foundry cheatcodes needed by Phase 1B without another dependency.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

interface Vm {
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
    function bound(uint256 value, uint256 minimum, uint256 maximum) external pure returns (uint256 result);
}

abstract contract TestBase {
    Vm internal constant vm = Vm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function _sameString(string memory left, string memory right) internal pure returns (bool) {
        return keccak256(bytes(left)) == keccak256(bytes(right));
    }
}
