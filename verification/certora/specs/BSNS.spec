using BSNS as token;

methods {
    function bridge() external returns (address) envfree;
    function totalSupply() external returns (uint256) envfree;
    function balanceOf(address) external returns (uint256) envfree;
    function authorizationState(address, bytes32) external returns (bool) envfree;
}

rule onlyBridgeChangesSupply(env e, method f, calldataarg args) {
    uint256 supplyBefore = totalSupply();
    f@withrevert(e, args);
    uint256 supplyAfter = totalSupply();

    assert supplyAfter > supplyBefore =>
        f.selector == sig:bridgeMint(address,uint256).selector && e.msg.sender == bridge();
    assert supplyAfter < supplyBefore =>
        f.selector == sig:bridgeBurn(uint256).selector && e.msg.sender == bridge();
}

rule transfersPreserveSupply(env e, method f, calldataarg args) filtered { f ->
    f.selector != sig:bridgeMint(address,uint256).selector &&
    f.selector != sig:bridgeBurn(uint256).selector
} {
    uint256 supplyBefore = totalSupply();
    f@withrevert(e, args);
    assert totalSupply() == supplyBefore, "non-bridge supply methods conserve total supply";
}

rule authorizationNonceNeverReopens(env e, method f, calldataarg args, address authorizer, bytes32 nonce) {
    require authorizationState(authorizer, nonce);
    f@withrevert(e, args);
    assert authorizationState(authorizer, nonce), "used or canceled nonces remain terminal";
}

rule authorizedTransferConsumesNonce(env e) {
    require e.msg.value == 0;

    address from;
    address to;
    uint256 value;
    uint256 validAfter;
    uint256 validBefore;
    bytes32 nonce;
    uint8 v;
    bytes32 r;
    bytes32 s;
    uint256 supplyBefore = totalSupply();
    uint256 fromBefore = balanceOf(from);
    uint256 toBefore = balanceOf(to);
    storage before = lastStorage;

    transferWithAuthorization@withrevert(e, from, to, value, validAfter, validBefore, nonce, v, r, s);

    satisfy !lastReverted, "a valid EIP-3009 transfer exists";
    if (lastReverted) {
        assert lastStorage[token] == before[token], "failed authorization is atomic";
    } else {
        assert authorizationState(from, nonce);
        assert totalSupply() == supplyBefore;
        assert to_mathint(balanceOf(from)) + value == fromBefore;
        assert to_mathint(balanceOf(to)) == toBefore + value;
    }
}

rule receiveAuthorizationRequiresRecipient(env e) {
    require e.msg.value == 0;

    address from;
    address to;
    uint256 value;
    uint256 validAfter;
    uint256 validBefore;
    bytes32 nonce;
    uint8 v;
    bytes32 r;
    bytes32 s;

    receiveWithAuthorization@withrevert(e, from, to, value, validAfter, validBefore, nonce, v, r, s);
    assert !lastReverted => e.msg.sender == to;
}

rule cancelConsumesNonce(env e) {
    require e.msg.value == 0;

    address authorizer;
    bytes32 nonce;
    uint8 v;
    bytes32 r;
    bytes32 s;
    storage before = lastStorage;

    cancelAuthorization@withrevert(e, authorizer, nonce, v, r, s);

    satisfy !lastReverted, "a valid cancellation exists";
    if (lastReverted) {
        assert lastStorage[token] == before[token], "failed cancellation is atomic";
    } else {
        assert authorizationState(authorizer, nonce);
    }
}

