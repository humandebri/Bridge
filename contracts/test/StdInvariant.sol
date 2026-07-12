// contracts/test: provide the minimal Foundry invariant target registry without importing forge-std.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

abstract contract StdInvariant {
    struct FuzzSelector {
        address addr;
        bytes4[] selectors;
    }

    struct FuzzArtifactSelector {
        string artifact;
        bytes4[] selectors;
    }

    struct FuzzInterface {
        address addr;
        string[] artifacts;
    }

    address[] private _targetedContracts;

    function targetContract(address target) internal {
        _targetedContracts.push(target);
    }

    function targetContracts() public view returns (address[] memory) {
        return _targetedContracts;
    }

    function targetSelectors() public pure returns (FuzzSelector[] memory selectors) {
        return new FuzzSelector[](0);
    }

    function targetSenders() public pure returns (address[] memory senders) {
        return new address[](0);
    }

    function targetArtifacts() public pure returns (string[] memory artifacts) {
        return new string[](0);
    }

    function targetArtifactSelectors() public pure returns (FuzzArtifactSelector[] memory selectors) {
        return new FuzzArtifactSelector[](0);
    }

    function targetInterfaces() public pure returns (FuzzInterface[] memory interfaces_) {
        return new FuzzInterface[](0);
    }

    function excludeContracts() public pure returns (address[] memory contracts_) {
        return new address[](0);
    }

    function excludeSelectors() public pure returns (FuzzSelector[] memory selectors) {
        return new FuzzSelector[](0);
    }

    function excludeSenders() public pure returns (address[] memory senders) {
        return new address[](0);
    }

    function excludeArtifacts() public pure returns (string[] memory artifacts) {
        return new string[](0);
    }
}
