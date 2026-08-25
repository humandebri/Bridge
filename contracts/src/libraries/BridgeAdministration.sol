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

    function serviceFeeIsValid(uint256 serviceFee, uint256 minimumServiceFee, uint256 maximumServiceFee)
        internal
        pure
        returns (bool)
    {
        return minimumServiceFee <= serviceFee && serviceFee <= maximumServiceFee;
    }

    function valueFitsU128(uint256 value) internal pure returns (bool) {
        return value <= type(uint128).max;
    }

    function valuesFitU128(uint256 first, uint256 second, uint256 third) internal pure returns (bool) {
        return valueFitsU128(first) && valueFitsU128(second) && valueFitsU128(third);
    }

    function timelockDelayIsValid(uint256 delay, uint256 minimumDelay, uint256 maximumDelay)
        internal
        pure
        returns (bool)
    {
        return minimumDelay <= delay && delay <= maximumDelay;
    }

    function timelockRoleIsClosed(address member, address requiredMember, bool memberHasRole, bool roleIsOpen)
        internal
        pure
        returns (bool)
    {
        return member != address(0) && (requiredMember == address(0) || member == requiredMember) && memberHasRole
            && !roleIsOpen;
    }

    function timelockHasNoPendingOperations(uint256 pendingOperationCount) internal pure returns (bool) {
        return pendingOperationCount == 0;
    }

    function withdrawalClaimAllowed(bool alreadyClaimed) internal pure returns (bool) {
        return !alreadyClaimed;
    }
}
