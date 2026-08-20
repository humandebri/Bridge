# Base Interface仕様

Phase 1Eで凍結するBase側のconcrete ABIとinterfaceを記録する。Solidity宣言は`contracts/src/interfaces/`と`contracts/src/`を正本とし、concrete ABI snapshotとselector fixtureで差分を検出する。

## Deployment

`Bridge`は次のconstructorを持ち、内部で`BSNS`を生成する。

```solidity
constructor(
    address initialBridgeSigner,
    address initialRuntimeAdministrator,
    address initialBaseAdminTimelock,
    bytes32 initialApprovedTimelockRuntimeCodeHash,
    uint256 initialPerDepositLimit,
    uint256 initialMintWindowLimit,
    uint64 initialMintWindowDuration,
    uint256 maxServiceFee,
    uint256 initialServiceFee
)
```

`Bridge`が生成する`BSNS`のERC-20 metadataは、`name = "KINIC"`、`symbol = "KINIC"`、`decimals = 8`にcontract内で固定する。constructorからはmetadataを受け取らず、異なるmetadataでdeployできない。`bKINIC`のような`b` prefixは付けない。`bSNS`はBridgeable SNS Tokenを表す内部の総称であり、token metadataには使用しない。

3個の権限addressはzero addressを禁止し、相互に異なる必要がある。limitとwindow durationはzeroを禁止し、`initialServiceFee <= maxServiceFee`を要求する。固定decimalsはKINIC Ledger `73mez-iiaaa-aaaaq-aaasq-cai`のdecimalsと同じ8である。`initialApprovedTimelockRuntimeCodeHash`は、deploy時およびTimelock rotation時に検証するOpenZeppelin Timelock runtime code hashである。

## EIP-3009署名送金

bSNSは標準ERC-20に加えて、次のEIP-3009 interfaceを提供する。

```solidity
function version() external pure returns (string memory); // "1"
function authorizationState(address authorizer, bytes32 nonce) external view returns (bool);
function transferWithAuthorization(
    address from,
    address to,
    uint256 value,
    uint256 validAfter,
    uint256 validBefore,
    bytes32 nonce,
    uint8 v,
    bytes32 r,
    bytes32 s
) external;
function receiveWithAuthorization(
    address from,
    address to,
    uint256 value,
    uint256 validAfter,
    uint256 validBefore,
    bytes32 nonce,
    uint8 v,
    bytes32 r,
    bytes32 s
) external;
function cancelAuthorization(address authorizer, bytes32 nonce, uint8 v, bytes32 r, bytes32 s) external;

event AuthorizationUsed(address indexed authorizer, bytes32 indexed nonce);
event AuthorizationCanceled(address indexed authorizer, bytes32 indexed nonce);
```

`validAfter`と`validBefore`はUnix timeであり、`block.timestamp > validAfter && block.timestamp < validBefore`の間だけ使用できる。使用済みと取消済みのnonceはauthorizerごとの単一namespaceで管理し、どちらのauthorization送金関数からも再利用できない。`receiveWithAuthorization`はcallerと`to`の一致を要求する。EIP-712 domainはtoken name、固定version `"1"`、実行chain ID、bSNS contract addressへ束縛し、EIP-5267 `eip712Domain()`から取得できる。

## Roles

| Role | 即時操作 | 禁止操作 |
|---|---|---|
| Bridge Signer | EIP-712 Mint Authorizationへの署名 | Base transaction送信、pause、limit・fee変更、role rotation、Withdrawal操作 |
| Runtime Administrator | Deposit/Withdrawal pause、上限内Service Fee変更 | unpause、limit変更、role rotation、mint |
| Base Admin Timelock | unpause、3権限addressのrotation | limit変更、直接mint、Withdrawal操作 |

任意のrole memberを追加できるgenericなgrant APIは公開しない。Bridge SignerとRuntime Administratorは常に単一addressとする。
rotationでもzero addressと3権限addressの重複を拒否し、初期deploy後の権限分離を維持する。

Base Admin TimelockにはOpenZeppelin 5.6.1の`TimelockController`を使用する。
Bridgeより先にdeployし、minimum delayを24時間、Canister由来Governance Operatorをproposer、executor、canceller、追加adminをzero addressとして初期化する。人間のEVM管理walletにはroleを付与しない。
Timelock自身が唯一のadminである。構築後のTimelock role集合は凍結し、`grantRole`、`revokeRole`、`renounceRole`を自己callを含めて拒否する。role変更が必要な場合は、新しい承認済みrole集合で同一runtimeのTimelockを配置し、BridgeのTimelock rotationを行う。
Bridgeはrotation候補のcode、24時間以上のdelay、Timelock自身のadmin保持を検証する。role分離はdeployment profileとdeploy preflightで確認する。

## Deposit mint

Canisterは次のEIP-712 payloadへthreshold ECDSA署名する。domainは`name = "KINIC Bridge"`、`version = "1"`、実行chain ID、Bridge contract addressへ束縛する。

```solidity
struct MintAuthorization {
    bytes32 depositId;
    address recipient;
    uint256 grossAmount;
    uint256 maxServiceFee;
    uint256 chargedServiceFee;
    uint256 deadline;
    uint256 authorizationEpoch;
}

function mintDepositWithAuthorization(
    MintAuthorization calldata authorization,
    bytes calldata signature
) external;
```

callerは制限しない。callerはgasだけを支払い、mint先は署名済み`recipient`から変更できない。Contractは`block.timestamp <= deadline`、`authorizationEpoch == mintAuthorizationEpoch`、EIP-712署名の復元addressが現在の`bridgeSigner`であることを検証する。OpenZeppelin `ECDSA.tryRecover`を使うため、不正長、不正`v`、high-s署名を拒否する。

`chargedServiceFee <= maxServiceFee`かつ`chargedServiceFee <= MAX_SERVICE_FEE`を検証し、実mint量`grossAmount - chargedServiceFee`へPer-Deposit LimitとMint Throughput Limitを適用する。受付後のglobal `serviceFee`変更は既存Authorizationのmint量とevent値へ影響しない。成功時は`DepositMinted`へEIP-712 digestをindexed fieldとして記録する。

各Depositは1件ずつmintする。zero recipient、不正amount、fee保護違反、Per-Deposit Limit違反、共有Mint Throughput Limit違反をrevertする。成功後の`depositId`は再利用できず、複数回のmintは同じfixed windowのthroughputへ累積する。

fixed windowはBridge deploy時刻から開始する。`block.timestamp >= mintWindowStartedAt + mintWindowDuration`となった後、最初に成功したmintの時刻を次windowの起点にし、消費量をresetする。失敗したmintは起点も消費量も変更しない。window境界直前と直後には最大2 window分をmintできるため、上限値は`docs/parameters.md`の2倍係数を前提に導出する。

`mintAuthorizationEpoch`は1から始まる。Deposit mintがactiveからpausedへ変わるとき、pausedからactiveへ戻るとき、またはBridge Signerが実際に別addressへrotationするときに1増加し、遷移前に作られた未期限Authorizationを一括失効する。repeated pause、repeated unpause、同じsignerへのrotationでは増加しない。

## Withdrawal

Withdrawal IDは1から始まるcontract内`uint256`連番とし、0を`None`用に予約する。未存在IDの`getWithdrawal`は`status = None`のdefault structを返す。ICRC-1 Accountはraw principalの`bytes owner`と`bytes32 subaccount`で保持し、zero subaccountをdefault subaccountとする。ownerは1〜29 bytesだけを許可し、空のmanagement principalとanonymous principal `hex"04"`を拒否する。

`createWithdrawal(amount, maxServiceFee, owner, subaccount)`は、burn前に現在の`serviceFee <= maxServiceFee`と`amount > serviceFee`を検証する。callerは事前にBridgeへ要求額ちょうどをapproveする。実行時は`transferFrom`、Bridge残高のburn、次の固定quoteを持つ`Committed` record作成を同一transactionで行い、`WithdrawalCommitted`を発行する。途中失敗はすべてrevertする。

```text
chargedServiceFee = 実行時のserviceFee
chargedServiceFee <= maxServiceFee
amountOut = amount - chargedServiceFee
```

Withdrawal stateは`None | Committed`だけであり、CommittedはBase上の不可逆な終端状態である。
`acknowledgeRelease`、`cancelRelease`、`refundWithdrawal`、Withdrawal専用remint、Ledger block情報はABIに存在しない。
burn後のICP側債務はCanisterが元のWithdrawal IDとIC Accountを維持して再試行、照合する。
この制約はBridge Signerに付与された通常のDeposit mint権限を取り消すものではなく、侵害されたSignerによる別の未処理Deposit IDのmintはmint throughput limitとpauseによって被害速度を制限する。

## Pauseと固定limit

Deposit mintとWithdrawal作成は独立してpauseする。pauseは既にCommittedとなったCanister債務の送金・照合を止めない。

Per-Deposit Limit、Mint Throughput Limit、window durationはconstructorで固定する。deploy後に変更するfunction、selector、管理経路は持たない。

Runtime Administratorは`serviceFee`をzeroからimmutableな`MAX_SERVICE_FEE`まで変更できる。
pause、unpause、Service Fee、role rotationは同じ状態または値への再実行を成功扱いにし、storageとeventを変更しない。
role rotation成立後は旧addressの権限を即時失効する。

## Phase境界

Phase 1DではService Fee変更、pause、固定limit、role rotationと24時間Timelock統合までを実装し、Phase 1Eではconcrete ABI、stateful invariant、SMT証明義務、coverage summaryを閉じる。
Baseにはfee reserveとFee Recipientを持たせない。
Phase 1E完了時点でconcrete Bridge・BSNS ABIをsnapshotとfixtureにより凍結する。現段階のcontractは本番資産を受け付けない。
