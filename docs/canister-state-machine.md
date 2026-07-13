# Bridge canister状態機械

## 境界

`bridge-core`は時刻、caller、IC API、Candid、stable structures、外部通信へ依存しない。
adapterがCandid `Nat`とBase `uint256`を`Amount(u128)`へchecked変換し、finalizedなBase stateから得たfee・limit・mint windowをevent入力として渡す。
`bridge-canister`は公開Deposit受付、状態照会、運用管理、ICRC Ledger、EVM RPC、threshold ECDSA、timer taskをこのcoreへ接続する。
公開Candidは`request_deposit`、DepositとWithdrawalの照会、Bridge status、pauseとresume、管理者rotation、Fee Recipient変更、fee payout、監査ログ照会を提供する。

## 遷移

- Deposit: `PullPending → Escrowed → MintPending → Minted`または`MintReverted`
- Depositの不明ledger結果: `PullPending → ReconciliationHold → Escrowed`または`Cancelled`
- Withdrawal release: `Observed → ReleasePending → ReleaseTransferred → AcknowledgePending → Released`または`AcknowledgeReverted`
- Withdrawal refund: `Observed → RefundPending → Refunded`または`RefundReverted`
- Withdrawalの不明ledger結果: `ReleasePending → ReconciliationHold → ReleaseTransferred`または新しいtransfer identityを持つ`ReleasePending`
- EVM操作: `Queued → Prepared → Submitted → Finalized`または`Reverted`
- Reconciliation Hold: `Open → ResolvedSucceeded`または`ResolvedAbsent`

同一eventのretryは現在のstateとpayloadが一致するときだけ冪等になる。
同一IDの受付payload hashが異なるretryはconflictとして拒否する。
`Released`と`Refunded`はterminalで相互遷移せず、Release開始後はRefund経路へ入れない。
Deposit Service Feeは受付時の単一finalized Base snapshotで固定し、Deposit mint確定時にだけ会計へ加算する。受付は同snapshotの実効window消費量、stableな未確定Mint予約量、新規net量をchecked加算し、limit超過をLedger pull前に拒否する。予約は`PullPending`から保持し、`Minted`または`Cancelled`でだけ解放する。
Reconciliation Holdはrequest種別・request ID・transfer identity・hold IDが一致する証拠付きresolutionだけを受理する。
成功証拠はledger block index、不存在証拠は完全履歴確認済みwatermarkを必須とし、request recordとHold recordを一体で遷移・保存する。
Holdから直接transfer、refund、補償へ遷移しない。
Depositの不存在が完全履歴scanで確定した場合はDeposit IDを再利用不能な`Cancelled`へ遷移させる。
Ledgerがallowance不足、残高不足、Bad Feeなど資産移動なしの確定失敗を返した場合も同じIDを`Cancelled`へ終端化し、Mint予約を直ちに解放する。temporarily unavailable、future timestamp、generic errorはretryable failureとして`PullPending`を維持し、確定取消と分離する。
Withdrawalの不存在が確定した場合だけ、経済的payloadを維持した新しいtransfer identityとattempt番号でReleaseを再開する。

adapterはcycles残高、signer ETH残高、gas予算からSettlement Reserveを算出する。
残高を観測できない場合またはreserveが不足する場合は、ICRC pull前に新規Depositだけを拒否し、既存Settlementの処理を継続する。
nonce未割当のEVM操作はacknowledgement、refund、deposit mintの順に割り当てる。

pause principalは新規Depositを停止できる。
Governance principalだけがDeposit受付の再開とruntime administrator rotationを実行できる。
finance administratorだけがFee Recipient変更とfee payoutを実行できる。
pause、resume、管理者rotation、Fee Recipient変更、fee payout、reserve gate、Base Service Fee観測変更はappend-only監査ログへ保存する。

`safe`到達はMemory ID 16のreorg可能なsidecar証跡へだけ保存する。primary operation、Deposit、Withdrawal、fee accounting、reserve、pauseは変更しない。safe headの後退、receipt消失、contract状態不一致をprovider合意で確認した場合はsidecarを削除し、同一raw transactionの監視を継続する。

finalized EVM revertはoperationと所有recordをterminalなReverted状態へ遷移させ、新規Depositを自動pauseする。未解決revertが存在する間はGovernanceによるresumeも拒否し、同一transactionを再送しない。既存Withdrawal settlement、Hold照合、receipt確認は継続する。

## Stable schema v5

| Memory ID | 内容 |
|---:|---|
| 0 | schema version cell |
| 1 | accounting cell |
| 2 | Deposit map |
| 3 | Withdrawal map |
| 4 | EVM operation map |
| 5 | Reconciliation Hold map |
| 6 | counters cell |
| 7 | EVM nonce、Withdrawal log cursor、last finalized Base block、last finalized Mint block |
| 8 | 署名前後のEIP-1559 transaction envelope |
| 9 | Reconciliation履歴scan progress |
| 10 | immutable init configuration |
| 11 | Deposit caller、client request ID、Base recipient |
| 12 | administrator state |
| 13 | append-only audit event map |
| 14 | fee payout map |
| 15 | nonce割当前のEVM call intent map |
| 16 | reorg可能なBase safe receipt・contract state観測map |
| 17〜31 | 予約済み、再利用禁止 |

本番未デプロイのためlegacy migrationは持たず、schema v5だけを受理する。
異なるschema versionはfail closedで拒否する。

各valueは先頭1 byteのwire versionとCBOR payloadからなる最大16 KiBのbounded blobである。
未知version、decode失敗、超過サイズ、未知schema versionはerrorまたはtrapとしてfail closedに扱い、default stateへ置換しない。
stateはstable structuresへrecord単位で直接保存し、`pre_upgrade`で一括serializeしない。
pending/reverted EVM操作、open Hold、pending Ledger操作、予約Mint量、audit sequence、fee payout IDのcounterはmemory ID 6へ保存する。
`get_bridge_status`は履歴mapを走査せず、counterとprogress cellから状態を返す。
