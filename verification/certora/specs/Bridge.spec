using Bridge as bridge;
using BSNS as token;

links {
    bridge.bsns => token;
}

methods {
    function bridgeSigner() external returns (address) envfree;
    function runtimeAdministrator() external returns (address) envfree;
    function baseAdminTimelock() external returns (address) envfree;
    function mintAuthorizationEpoch() external returns (uint256) envfree;
    function serviceFee() external returns (uint256) envfree;
    function MIN_SERVICE_FEE() external returns (uint256) envfree;
    function MAX_SERVICE_FEE() external returns (uint256) envfree;
    function mintWindowStartedAt() external returns (uint64) envfree;
    function mintWindowDuration() external returns (uint64) envfree;
    function mintedInWindow() external returns (uint256) envfree;
    function nextWithdrawalId() external returns (uint256) envfree;
    function depositMintsPaused() external returns (bool) envfree;
    function isDepositProcessed(bytes32) external returns (bool) envfree;
    function token.totalSupply() external returns (uint256) envfree;
    function token.balanceOf(address) external returns (uint256) envfree;
}

definition rolesAreSafe() returns bool =
    bridgeSigner() != 0 &&
    runtimeAdministrator() != 0 &&
    baseAdminTimelock() != 0 &&
    bridgeSigner() != runtimeAdministrator() &&
    bridgeSigner() != baseAdminTimelock() &&
    runtimeAdministrator() != baseAdminTimelock();

definition feeIsSafe() returns bool =
    MIN_SERVICE_FEE() <= serviceFee() && serviceFee() <= MAX_SERVICE_FEE();

ghost bool depositMintedEventSeen;
ghost bytes32 depositMintedEventDepositId;
ghost bytes32 depositMintedEventRecipient;

ghost bool withdrawalCommittedEventSeen;
ghost bytes32 withdrawalCommittedEventId;
ghost bytes32 withdrawalCommittedEventRequester;

ghost mapping(bytes32 => bool) processedTrueRead;

hook LOG4(uint offset, uint length, bytes32 topic0, bytes32 topic1, bytes32 topic2, bytes32 topic3) {
    if (
        executingContract == currentContract &&
        topic0 == to_bytes32(0xa1000a97ca9a256d5f45e3de4e932f54cc70351ba99011741ebbf0241cb64f87)
    ) {
        depositMintedEventSeen = true;
        depositMintedEventDepositId = topic1;
        depositMintedEventRecipient = topic2;
    }
}

hook LOG3(uint offset, uint length, bytes32 topic0, bytes32 topic1, bytes32 topic2) {
    if (
        executingContract == currentContract &&
        topic0 == to_bytes32(0x0f17d1cf1a9ab4ddf68e3e00e13e5f865f52290dd33ee3872fe6a4f1aabf929c)
    ) {
        withdrawalCommittedEventSeen = true;
        withdrawalCommittedEventId = topic1;
        withdrawalCommittedEventRequester = topic2;
    }
}

hook Sload bool value _processedDeposits[KEY bytes32 depositId] {
    if (value) {
        processedTrueRead[depositId] = true;
    }
}

rule mintAppliesExactEffects(env e) {
    require e.msg.value == 0;

    IBridge.MintAuthorization authorization;
    bytes signature;
    uint256 supplyBefore = token.totalSupply();
    uint256 recipientBalanceBefore = token.balanceOf(authorization.recipient);
    uint256 consumedBefore = mintedInWindow();
    uint64 windowStartedBefore = mintWindowStartedAt();
    uint64 windowDuration = mintWindowDuration();
    bool processedBefore = currentContract._processedDeposits[authorization.depositId];
    storage bridgeBefore = lastStorage;

    depositMintedEventSeen = false;
    mintDepositWithAuthorization@withrevert(e, authorization, signature);

    satisfy !lastReverted, "an authorized mint reaches the commit boundary";
    if (lastReverted) {
        assert lastStorage[bridge] == bridgeBefore[bridge], "failed mint preserves Bridge storage";
        assert lastStorage[token] == bridgeBefore[token], "failed mint preserves token storage";
    } else {
        mathint netAmount = to_mathint(authorization.grossAmount) - authorization.chargedServiceFee;
        assert !processedBefore;
        assert currentContract._processedDeposits[authorization.depositId];
        assert to_mathint(token.totalSupply()) == supplyBefore + netAmount;
        assert to_mathint(token.balanceOf(authorization.recipient)) == recipientBalanceBefore + netAmount;
        assert depositMintedEventSeen, "successful mint emits DepositMinted";
        assert depositMintedEventDepositId == authorization.depositId;
        assert depositMintedEventRecipient == to_bytes32(authorization.recipient);

        if (to_mathint(e.block.timestamp) >= to_mathint(windowStartedBefore) + windowDuration) {
            assert to_mathint(mintWindowStartedAt()) == e.block.timestamp;
            assert to_mathint(mintedInWindow()) == netAmount;
        } else {
            assert mintWindowStartedAt() == windowStartedBefore;
            assert to_mathint(mintedInWindow()) == consumedBefore + netAmount;
        }
    }
}

rule processedDepositCannotReopen(env e) {
    require e.msg.value == 0;

    IBridge.MintAuthorization authorization;
    bytes signature;
    require currentContract._processedDeposits[authorization.depositId];
    processedTrueRead[authorization.depositId] = false;
    storage before = lastStorage;

    mintDepositWithAuthorization@withrevert(e, authorization, signature);

    assert lastReverted, "a processed deposit cannot mint again";
    assert lastStorage[bridge] == before[bridge];
    assert lastStorage[token] == before[token];
    satisfy processedTrueRead[authorization.depositId], "a signature-valid path reaches the replay guard";
}

rule withdrawalBurnAndRecordAreAtomic(env e) {
    require e.msg.value == 0;

    uint256 amount;
    uint256 maximumFee;
    bytes owner;
    bytes32 subaccount;
    uint256 idBefore = nextWithdrawalId();
    uint256 supplyBefore = token.totalSupply();
    uint256 chargedFeeBefore = serviceFee();
    storage before = lastStorage;

    withdrawalCommittedEventSeen = false;
    createWithdrawal@withrevert(e, amount, maximumFee, owner, subaccount);

    satisfy !lastReverted, "a funded and approved withdrawal can commit";
    if (lastReverted) {
        assert lastStorage[bridge] == before[bridge], "failed withdrawal preserves Bridge storage";
        assert lastStorage[token] == before[token], "failed withdrawal preserves token storage";
    } else {
        assert nextWithdrawalId() == idBefore + 1;
        assert to_mathint(token.totalSupply()) + amount == supplyBefore;
        assert currentContract._withdrawals[idBefore].exists;
        assert currentContract._withdrawals[idBefore].requester == e.msg.sender;
        assert currentContract._withdrawals[idBefore].amount == amount;
        assert currentContract._withdrawals[idBefore].maxServiceFee == maximumFee;
        assert currentContract._withdrawals[idBefore].chargedServiceFee == chargedFeeBefore;
        assert currentContract._withdrawals[idBefore].subaccount == subaccount;
        assert withdrawalCommittedEventSeen, "successful withdrawal emits WithdrawalCommitted";
        assert withdrawalCommittedEventId == to_bytes32(idBefore);
        assert withdrawalCommittedEventRequester == to_bytes32(e.msg.sender);
    }
}

rule administrationPreservesSafety(env e, method f, calldataarg args) {
    require rolesAreSafe();
    require feeIsSafe();
    require mintAuthorizationEpoch() > 0;

    uint256 epochBefore = mintAuthorizationEpoch();
    f@withrevert(e, args);

    assert rolesAreSafe(), "roles remain nonzero and pairwise distinct";
    assert feeIsSafe(), "service fee remains within immutable bounds";
    assert mintAuthorizationEpoch() >= epochBefore, "authorization epoch never decreases";
}

rule retiredSignerCannotBeReappointed(env e) {
    require e.msg.value == 0;

    address retiredSigner;
    require currentContract._retiredBridgeSigners[retiredSigner];
    storage before = lastStorage;

    rotateBridgeSigner@withrevert(e, retiredSigner);

    assert lastReverted, "a retired bridge signer cannot be appointed again";
    assert lastStorage[bridge] == before[bridge];
}

rule signerRotationRetiresPreviousAndAdvancesEpoch(env e) {
    require e.msg.value == 0;

    address newSigner;
    address previousSigner = bridgeSigner();
    uint256 previousEpoch = mintAuthorizationEpoch();

    rotateBridgeSigner@withrevert(e, newSigner);

    satisfy !lastReverted && newSigner != previousSigner,
        "a valid base-admin signer rotation exists";
    if (!lastReverted && newSigner != previousSigner) {
        assert bridgeSigner() == newSigner;
        assert currentContract._retiredBridgeSigners[previousSigner];
        assert !currentContract._retiredBridgeSigners[newSigner];
        assert mintAuthorizationEpoch() == previousEpoch + 1;
    }
    assert lastReverted || newSigner == previousSigner || bridgeSigner() == newSigner;
}

rule unpauseRequiresSignerRotationAfterPause(env e) {
    require e.msg.value == 0;

    address signerAtLastPause = currentContract._bridgeSignerAtLastPause;
    unpauseDepositMints@withrevert(e);

    if (!lastReverted) {
        assert !depositMintsPaused();
        assert signerAtLastPause == 0 || bridgeSigner() != signerAtLastPause,
            "a non-initial pause cannot be lifted with the same signer";
    }
    assert lastReverted || !depositMintsPaused();
}

rule processedFlagIsMonotone(env e, method f, calldataarg args, bytes32 depositId) {
    require currentContract._processedDeposits[depositId];
    f@withrevert(e, args);
    assert currentContract._processedDeposits[depositId], "processed deposits never reopen";
}
