// contracts/src/policies/staging: test-only deployment invariants selected only by the staging Foundry profile.
// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

library DeploymentPolicy {
    uint256 internal constant MINIMUM_TIMELOCK_DELAY = 5 minutes;
}
