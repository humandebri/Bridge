// verification/smt/pass: prove production administration predicates preserve role separation and fee bounds.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

import {BridgeAdministration} from "bridge-src/libraries/BridgeAdministration.sol";

contract BridgeAdministrationState {
    function separatedRoles(address bridgeSigner, address runtimeAdministrator, address baseAdminTimelock)
        external
        pure
    {
        bool nonzero = BridgeAdministration.rolesAreNonzero(
            bridgeSigner, runtimeAdministrator, baseAdminTimelock
        );
        bool distinct = BridgeAdministration.rolesAreDistinct(
            bridgeSigner, runtimeAdministrator, baseAdminTimelock
        );
        if (nonzero && distinct) {
            assert(bridgeSigner != address(0));
            assert(runtimeAdministrator != address(0));
            assert(baseAdminTimelock != address(0));
            assert(bridgeSigner != runtimeAdministrator);
            assert(bridgeSigner != baseAdminTimelock);
            assert(runtimeAdministrator != baseAdminTimelock);
        }
    }

    function boundedServiceFee(uint256 serviceFee, uint256 minimumServiceFee, uint256 maximumServiceFee)
        external
        pure
    {
        if (BridgeAdministration.serviceFeeIsValid(serviceFee, minimumServiceFee, maximumServiceFee)) {
            assert(minimumServiceFee <= serviceFee);
            assert(serviceFee <= maximumServiceFee);
        }
    }

    function boundedCrossSystemValues(uint256 first, uint256 second, uint256 third) external pure {
        if (BridgeAdministration.valuesFitU128(first, second, third)) {
            assert(first <= type(uint128).max);
            assert(second <= type(uint128).max);
            assert(third <= type(uint128).max);
        }
    }

    function safeTimelockDelay(uint256 delay, uint256 minimumDelay, uint256 maximumDelay) external pure {
        if (BridgeAdministration.timelockDelayIsValid(delay, minimumDelay, maximumDelay)) {
            assert(minimumDelay <= delay);
            assert(delay <= maximumDelay);
        }
    }

    function closedTimelockRole(
        address member,
        address requiredMember,
        bool memberHasRole,
        bool roleIsOpen
    ) external pure {
        if (BridgeAdministration.timelockRoleIsClosed(member, requiredMember, memberHasRole, roleIsOpen)) {
            assert(member != address(0));
            assert(requiredMember == address(0) || member == requiredMember);
            assert(memberHasRole);
            assert(!roleIsOpen);
        }
    }

    function noPendingTimelockOperations(uint256 pendingOperationCount) external pure {
        if (BridgeAdministration.timelockHasNoPendingOperations(pendingOperationCount)) {
            assert(pendingOperationCount == 0);
        }
    }

    function withdrawalClaimIsSingleUse(bool alreadyClaimed) external pure {
        if (BridgeAdministration.withdrawalClaimAllowed(alreadyClaimed)) {
            assert(!alreadyClaimed);
            assert(!BridgeAdministration.withdrawalClaimAllowed(true));
        }
    }
}
