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

rule pendingCountChangesOnlyThroughLifecycle(env e, method f, calldataarg args) {
    uint256 before = pendingOperationCount();
    f@withrevert(e, args);
    uint256 after = pendingOperationCount();

    assert after > before =>
        f.selector == sig:schedule(address,uint256,bytes,bytes32,bytes32,uint256).selector ||
        f.selector == sig:scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256).selector;
    assert after < before =>
        f.selector == sig:cancel(bytes32).selector ||
        f.selector == sig:execute(address,uint256,bytes,bytes32,bytes32).selector ||
        f.selector == sig:executeBatch(address[],uint256[],bytes[],bytes32,bytes32).selector;
}

rule scheduleChangesPendingCountExactly(env e) {
    require e.msg.value == 0;

    address target;
    uint256 value;
    bytes data;
    bytes32 predecessor;
    bytes32 salt;
    uint256 delay;
    uint256 before = pendingOperationCount();
    storage stateBefore = lastStorage;

    schedule@withrevert(e, target, value, data, predecessor, salt, delay);
    if (lastReverted) {
        assert lastStorage[timelock] == stateBefore[timelock];
    } else {
        assert pendingOperationCount() == before + 1;
    }
}

rule scheduleBatchChangesPendingCountExactly(env e) {
    require e.msg.value == 0;

    address[] targets;
    uint256[] values;
    bytes[] payloads;
    bytes32 predecessor;
    bytes32 salt;
    uint256 delay;
    uint256 before = pendingOperationCount();
    storage stateBefore = lastStorage;

    scheduleBatch@withrevert(e, targets, values, payloads, predecessor, salt, delay);
    if (lastReverted) {
        assert lastStorage[timelock] == stateBefore[timelock];
    } else {
        assert pendingOperationCount() == before + 1;
    }
}

rule cancelChangesPendingCountExactly(env e) {
    require e.msg.value == 0;

    bytes32 id;
    uint256 before = pendingOperationCount();
    storage stateBefore = lastStorage;

    cancel@withrevert(e, id);
    if (lastReverted) {
        assert lastStorage[timelock] == stateBefore[timelock];
    } else {
        assert before > 0;
        assert pendingOperationCount() + 1 == before;
    }
}

rule executeChangesPendingCountExactly(env e) {
    address target;
    uint256 value;
    bytes payload;
    bytes32 predecessor;
    bytes32 salt;
    uint256 before = pendingOperationCount();
    storage stateBefore = lastStorage;

    execute@withrevert(e, target, value, payload, predecessor, salt);
    if (lastReverted) {
        assert lastStorage[timelock] == stateBefore[timelock];
    } else {
        assert before > 0;
        assert pendingOperationCount() + 1 == before;
    }
}

rule executeBatchChangesPendingCountExactly(env e) {
    address[] targets;
    uint256[] values;
    bytes[] payloads;
    bytes32 predecessor;
    bytes32 salt;
    uint256 before = pendingOperationCount();
    storage stateBefore = lastStorage;

    executeBatch@withrevert(e, targets, values, payloads, predecessor, salt);
    if (lastReverted) {
        assert lastStorage[timelock] == stateBefore[timelock];
    } else {
        assert before > 0;
        assert pendingOperationCount() + 1 == before;
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
