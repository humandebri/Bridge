using BridgeTimelockController as timelock;

methods {
    function PROPOSER_ROLE() external returns (bytes32) envfree;
    function CANCELLER_ROLE() external returns (bytes32) envfree;
    function EXECUTOR_ROLE() external returns (bytes32) envfree;
    function DEFAULT_ADMIN_ROLE() external returns (bytes32) envfree;
    function roleMember(bytes32) external returns (address) envfree;
    function hasRole(bytes32,address) external returns (bool) envfree;
    function getMinDelay() external returns (uint256) envfree;
    function MINIMUM_DELAY() external returns (uint256) envfree;
    function MAXIMUM_DELAY() external returns (uint256) envfree;
    function pendingOperationCount() external returns (uint256) envfree;

    unresolved external in BridgeTimelockController.execute(address,uint256,bytes,bytes32,bytes32)
        => DISPATCH [ TimelockNestedOperationTarget._ ] default HAVOC_ECF;
    unresolved external in BridgeTimelockController.executeBatch(address[],uint256[],bytes[],bytes32,bytes32)
        => DISPATCH [ TimelockNestedOperationTarget._ ] default HAVOC_ECF;
}

definition operationalRolePolicy(address account) returns bool =
    roleMember(DEFAULT_ADMIN_ROLE()) == currentContract &&
    roleMember(PROPOSER_ROLE()) != 0 &&
    roleMember(CANCELLER_ROLE()) != 0 &&
    roleMember(EXECUTOR_ROLE()) == roleMember(PROPOSER_ROLE()) &&
    roleMember(CANCELLER_ROLE()) != roleMember(PROPOSER_ROLE()) &&
    hasRole(PROPOSER_ROLE(), account) == (account == roleMember(PROPOSER_ROLE())) &&
    hasRole(EXECUTOR_ROLE(), account) == (account == roleMember(EXECUTOR_ROLE())) &&
    hasRole(CANCELLER_ROLE(), account) == (account == roleMember(CANCELLER_ROLE())) &&
    hasRole(DEFAULT_ADMIN_ROLE(), account) == (account == currentContract);

definition isPendingTimestamp(uint256 timestamp) returns bool = timestamp > 1;

ghost mathint scheduledTransitions;
ghost mathint terminalTransitions;
ghost mathint counterIncrements;
ghost mathint counterDecrements;

hook Sstore _timestamps[KEY bytes32 id] uint256 newTimestamp (uint256 oldTimestamp) {
    if (executingContract == currentContract) {
        if (oldTimestamp == 0 && isPendingTimestamp(newTimestamp)) {
            scheduledTransitions = scheduledTransitions + 1;
        } else if (isPendingTimestamp(oldTimestamp) && (newTimestamp == 0 || newTimestamp == 1)) {
            terminalTransitions = terminalTransitions + 1;
        } else if (oldTimestamp != newTimestamp) {
            assert false, "every timestamp write is a recognized operation lifecycle transition";
        }
    }
}

hook Sstore pendingOperationCount uint256 newCount (uint256 oldCount) {
    if (executingContract == currentContract && newCount != oldCount) {
        if (newCount > oldCount) {
            assert newCount == oldCount + 1, "a counter increment is exactly one";
            counterIncrements = counterIncrements + 1;
        } else {
            assert oldCount == newCount + 1, "a counter decrement is exactly one";
            counterDecrements = counterDecrements + 1;
        }
    }
}

rule operationalRolesRemainClosed(env e, method f, calldataarg args, address account) {
    require operationalRolePolicy(account);
    f@withrevert(e, args);
    assert operationalRolePolicy(account), "frozen operational roles remain single-member and closed";
}

rule delayAlwaysRemainsBounded(env e, method f, calldataarg args) {
    require MINIMUM_DELAY() <= getMinDelay() && getMinDelay() <= MAXIMUM_DELAY();
    f@withrevert(e, args);
    assert MINIMUM_DELAY() <= getMinDelay() && getMinDelay() <= MAXIMUM_DELAY();
}

rule pendingCounterTracksLifecycleTransitions(env e, method f, calldataarg args) {
    uint256 countBefore = pendingOperationCount();
    storage stateBefore = lastStorage;
    scheduledTransitions = 0;
    terminalTransitions = 0;
    counterIncrements = 0;
    counterDecrements = 0;

    f@withrevert(e, args);

    if (lastReverted) {
        assert lastStorage[timelock] == stateBefore[timelock],
            "a reverted lifecycle call preserves all timelock storage";
    } else {
        assert counterIncrements == scheduledTransitions,
            "every pending operation creation has one counter increment";
        assert counterDecrements == terminalTransitions,
            "every cancel or execute transition has one counter decrement";
        assert to_mathint(pendingOperationCount()) ==
            to_mathint(countBefore) + counterIncrements - counterDecrements,
            "the counter net change equals all nested lifecycle transitions";
    }
}

rule onlySelfCanUpdateDelay(env e) {
    require e.msg.value == 0;

    uint256 newDelay;
    updateDelay@withrevert(e, newDelay);
    assert !lastReverted =>
        e.msg.sender == currentContract &&
        MINIMUM_DELAY() <= newDelay && newDelay <= MAXIMUM_DELAY();
}
