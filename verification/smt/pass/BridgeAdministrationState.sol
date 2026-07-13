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

    function boundedServiceFee(uint256 serviceFee, uint256 maximumServiceFee) external pure {
        if (BridgeAdministration.serviceFeeIsValid(serviceFee, maximumServiceFee)) {
            assert(serviceFee <= maximumServiceFee);
        }
    }
}
