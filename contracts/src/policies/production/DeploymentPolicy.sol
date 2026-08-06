// contracts/src/policies/production: production deployment invariants selected by the default Foundry profile.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

library DeploymentPolicy {
    uint256 internal constant MINIMUM_TIMELOCK_DELAY = 24 hours;
}
