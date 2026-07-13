// contracts/src/libraries: share Base administration safety predicates with Bridge and SMT fixtures.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

library BridgeAdministration {
    function rolesAreNonzero(address bridgeSigner, address runtimeAdministrator, address baseAdminTimelock)
        internal
        pure
        returns (bool)
    {
        return bridgeSigner != address(0) && runtimeAdministrator != address(0) && baseAdminTimelock != address(0);
    }

    function rolesAreDistinct(address bridgeSigner, address runtimeAdministrator, address baseAdminTimelock)
        internal
        pure
        returns (bool)
    {
        return bridgeSigner != runtimeAdministrator && bridgeSigner != baseAdminTimelock
            && runtimeAdministrator != baseAdminTimelock;
    }

    function limitsAreNonzero(uint256 perDepositLimit, uint256 mintWindowLimit, uint64 mintWindowDuration)
        internal
        pure
        returns (bool)
    {
        return perDepositLimit != 0 && mintWindowLimit != 0 && mintWindowDuration != 0;
    }

    function serviceFeeIsValid(uint256 serviceFee, uint256 maximumServiceFee) internal pure returns (bool) {
        return serviceFee <= maximumServiceFee;
    }
}
