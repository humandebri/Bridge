# Base Interface仕様

Phase 1Aで確定したBase側の暫定interfaceを記録する。Solidity宣言は`contracts/src/interfaces/`を正本とし、Phase 1Eまでは意図的な変更を許可する。

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

KINIC用deployではERC-20 metadataを`name = "kinic"`、`symbol = "KINIC"`とする。`bKINIC`のような`b` prefixは付けない。`bSNS`はBridgeable SNS Tokenを表す内部の総称であり、token metadataには使用しない。

3個の権限addressはzero addressを禁止し、相互に異なる必要がある。limitとwindow durationはzeroを禁止し、`initialServiceFee <= maxServiceFee`を要求する。`tokenDecimals`は対象SNS ledgerのdecimalsと一致させるが、本番値の確定はPhase 6開始条件とする。

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
| Runtime Administrator | Deposit/Withdrawal pause、limit引下げ、window延長、上限内Service Fee変更 | unpause、limit引上げ、role rotation、mint、refund |
| Base Admin Timelock | unpause、任意limit変更、window短縮、3権限addressのrotation | 直接mint、直接refund |

任意のrole memberを追加できるgenericなgrant APIは公開しない。Bridge SignerとRuntime Administratorは常に単一addressとする。
rotationでもzero addressと3権限addressの重複を拒否し、初期deploy後の権限分離を維持する。

## Deposit mint

`DepositMintRequest`は`depositId`、Base recipient、ICPでlockした`grossAmount`、利用者指定の`maxServiceFee`を保持する。実mint量は`grossAmount - serviceFee`であり、Per-Deposit LimitとMint Throughput Limitはこの実mint量へ適用する。

singleとbatchは同じstructを使用する。batchはatomicであり、空batch、処理済みID、batch内重複、zero recipient、不正amount、fee保護違反、各Depositのlimit違反、batch合計のthroughput違反のいずれかで全体をrevertする。成功後の`depositId`は再利用できない。

fixed windowはBridge deploy時刻から開始する。`block.timestamp >= mintWindowStartedAt + mintWindowDuration`となった後、最初に成功したmintの時刻を次windowの起点にし、消費量をresetする。失敗したmintは起点も消費量も変更しない。window境界直前と直後には最大2 window分をmintできるため、上限値は`docs/parameters.md`の2倍係数を前提に導出する。

## Withdrawal

Withdrawal IDは1から始まるcontract内`uint256`連番とし、0を`None`用に予約する。ICRC-1 Accountはraw principalの`bytes owner`と`bytes32 subaccount`で保持し、zero subaccountをdefault subaccountとする。空principal、29 bytes超、anonymous principalは受取先として拒否する。

`createWithdrawal`はcallerのbSNSを全量burnし、recordを`Pending`にする。Release acknowledgementは次をすべて満たす必要がある。

```text
amountOut + serviceFee + ledgerFee == amount
amountOut >= minAmountOut
serviceFee <= MAX_SERVICE_FEE
```

acknowledgementはamount、fee、ledger block indexをrecordへ保存する。同一内容の再実行は成功するがeventやfeeを重複計上しない。異なる内容の再実行と`Refunded`へのacknowledgementはrevertする。Base Refundは`Pending`だけに許可し、元requesterへburn量全体を再mintする。

## Pauseとlimit変更

Deposit mintとWithdrawal作成は独立してpauseする。Release acknowledgementとBase Refundは既存Settlementを完了する操作であるためpauseの影響を受けない。

Runtime Administratorの`reduceMintLimits`は、Per-Deposit Limitとwindow limitを現行値以下、window durationを現行値以上に限定し、少なくとも1項目を安全方向へ変更する。Base Admin Timelockの`setMintLimits`はzero以外の任意値を設定できる。どちらも現在windowの開始時刻と消費量をresetしない。

## Phase境界

Phase 1BではbSNS、EIP-3009、Deposit mint、Per-Deposit Limit、fixed-window Mint Throughput Limitまでを実装した。WithdrawalとService Fee変更はPhase 1C、管理権限とtimelockはPhase 1Dで実装する。未実装APIのrevert stubは置かず、interfaceはPhase 1Eまで暫定とする。Phase 1B contractは本番資産を受け付けない。
