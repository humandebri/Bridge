# Base Interface仕様

Phase 1Eで凍結するBase側のconcrete ABIとinterfaceを記録する。Solidity宣言は`contracts/src/interfaces/`と`contracts/src/`を正本とし、concrete ABI snapshotとselector fixtureで差分を検出する。

## Deployment

`Bridge`は次のconstructorを持ち、内部で`BSNS`を生成する。

```solidity
constructor(
    string memory tokenName,
    string memory tokenSymbol,
    uint8 tokenDecimals,
    address bridgeSigner,
    address runtimeAdministrator,
    address baseAdminTimelock,
    uint256 perDepositLimit,
    uint256 mintWindowLimit,
    uint64 mintWindowDuration,
    uint256 maxServiceFee,
    uint256 initialServiceFee
)
```

KINIC用deployではERC-20 metadataを`name = "kinic"`、`symbol = "KINIC"`、`decimals = 8`とする。`bKINIC`のような`b` prefixは付けない。`bSNS`はBridgeable SNS Tokenを表す内部の総称であり、token metadataには使用しない。

3個の権限addressはzero addressを禁止し、相互に異なる必要がある。limitとwindow durationはzeroを禁止し、`initialServiceFee <= maxServiceFee`を要求する。`tokenDecimals`はKINIC Ledger `73mez-iiaaa-aaaaq-aaasq-cai`のdecimalsと同じ8に固定する。

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
| Bridge Signer | deposit mint、Release acknowledgement、Base Refund | pause、limit・fee変更、role rotation |
| Runtime Administrator | Deposit/Withdrawal pause、上限内Service Fee変更 | unpause、limit変更、role rotation、mint、refund |
| Base Admin Timelock | unpause、3権限addressのrotation | limit変更、直接mint、直接refund |

任意のrole memberを追加できるgenericなgrant APIは公開しない。Bridge SignerとRuntime Administratorは常に単一addressとする。
rotationでもzero addressと3権限addressの重複を拒否し、初期deploy後の権限分離を維持する。

Base Admin TimelockにはOpenZeppelin 5.6.1の`TimelockController`を使用する。
Bridgeより先にdeployし、minimum delayを72時間、単一のBase Admin hardware walletだけをproposer、canceller、executor、追加adminをzero addressとして初期化する。
Timelock自身が唯一のadminであり、delayとTimelock roleの変更もschedule済み自己callだけに許可する。
Bridgeはこの構成を内部検証しないため、deploy preflightで確認する。

## Deposit mint

`DepositMintRequest`は`depositId`、Base recipient、ICPでlockした`grossAmount`、利用者指定の`maxServiceFee`、受付時に固定した`chargedServiceFee`を保持する。Contractは`chargedServiceFee <= maxServiceFee`かつ`chargedServiceFee <= MAX_SERVICE_FEE`を検証し、実mint量`grossAmount - chargedServiceFee`へPer-Deposit LimitとMint Throughput Limitを適用する。受付後のglobal `serviceFee`変更は既存Depositのmint量とevent値へ影響しない。

singleとbatchは同じstructを使用する。batchはatomicであり、空batch、処理済みID、batch内重複、zero recipient、不正amount、fee保護違反、各Depositのlimit違反、batch合計のthroughput違反のいずれかで全体をrevertする。成功後の`depositId`は再利用できない。

fixed windowはBridge deploy時刻から開始する。`block.timestamp >= mintWindowStartedAt + mintWindowDuration`となった後、最初に成功したmintの時刻を次windowの起点にし、消費量をresetする。失敗したmintは起点も消費量も変更しない。window境界直前と直後には最大2 window分をmintできるため、上限値は`docs/parameters.md`の2倍係数を前提に導出する。

## Withdrawal

Withdrawal IDは1から始まるcontract内`uint256`連番とし、0を`None`用に予約する。未存在IDの`getWithdrawal`は`status = None`のdefault structを返す。ICRC-1 Accountはraw principalの`bytes owner`と`bytes32 subaccount`で保持し、zero subaccountをdefault subaccountとする。ownerは1〜29 bytesだけを許可し、空のmanagement principalとanonymous principal `hex"04"`を拒否する。

`createWithdrawal`は`amount > 0`と`1 <= minAmountOut <= amount`を要求し、callerのbSNSを全量burnしてrecordを`Pending`にする。現在のService Feeと未知のledger feeを使ったcreate時の実行可能性判定は行わない。Release acknowledgementは次をすべて満たす必要がある。

```text
amountOut + serviceFee + ledgerFee == amount
amountOut >= minAmountOut
serviceFee <= MAX_SERVICE_FEE
```

acknowledgementはamountOut、fee、ledger block indexをrecordへ保存する。acknowledgementのfeeは実行時の`serviceFee`と一致する必要はない。同一内容の再実行は成功するがeventやfeeを重複記録しない。異なる内容の再実行と`Refunded`へのacknowledgementはrevertする。同じledger block indexを別Withdrawalへ使用することも拒否する。Base Refundは`Pending`だけに許可し、元requesterへburn量全体を再mintする。refundではfeeを記録せず、Deposit mint windowを消費しない。

## Pauseと固定limit

Deposit mintとWithdrawal作成は独立してpauseする。Release acknowledgementとBase Refundは既存Settlementを完了する操作であるためpauseの影響を受けない。

Per-Deposit Limit、Mint Throughput Limit、window durationはconstructorで固定する。deploy後に変更するfunction、selector、管理経路は持たない。

Runtime Administratorは`serviceFee`をzeroからimmutableな`MAX_SERVICE_FEE`まで変更できる。
pause、unpause、Service Fee、role rotationは同じ状態または値への再実行を成功扱いにし、storageとeventを変更しない。
role rotation成立後は旧addressの権限を即時失効する。

## Phase境界

Phase 1DではService Fee変更、pause、固定limit、role rotationと72時間Timelock統合までを実装し、Phase 1Eではconcrete ABI、stateful invariant、SMT証明義務、coverage summaryを閉じる。
Baseにはfee reserveとFee Recipientを持たせない。
Phase 1E完了時点でconcrete Bridge・BSNS ABIをsnapshotとfixtureにより凍結する。現段階のcontractは本番資産を受け付けない。
