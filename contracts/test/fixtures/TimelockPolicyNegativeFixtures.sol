// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.36;

library ProductionTimelockPolicyNegativeFixture {
    uint256 internal constant MINIMUM_DELAY = 24 hours;
    uint256 internal constant REJECTED_DELAY = MINIMUM_DELAY - 1;
}

library StagingTimelockPolicyNegativeFixture {
    uint256 internal constant MINIMUM_DELAY = 5 minutes;
    uint256 internal constant REJECTED_DELAY = MINIMUM_DELAY - 1;
}
