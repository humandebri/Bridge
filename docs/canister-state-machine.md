# Bridge canister状態機械

## 境界

`bridge-core`は時刻、caller、IC API、Candid、stable structures、外部通信へ依存しない。
adapterがCandid `Nat`とBase `uint256`を`Amount(u128)`へchecked変換し、safeなBase stateから得たfee・limit・mint windowをevent入力として渡す。
`bridge-canister`は公開Deposit受付、状態照会、運用管理、ICRC Ledger、EVM RPC、threshold ECDSA、stable confirmation schedulerをこのcoreへ接続する。schedulerは最短scheduleだけをone-shot timerへ登録し、同時に1 recordずつ処理する。
公開Candidは`get_next_deposit_sequence`、`request_deposit`、`notify_withdrawal`、`continue_deposit`、`continue_withdrawal`、`continue_fee_payout`、DepositとWithdrawalの照会、Bridge status、pauseとresume、管理者rotation、Fee Recipient変更、fee payout、監査ログ照会を提供する。

## 遷移

- Deposit: `PullPending → Escrowed → MintPending → Minted`または`MintReverted`
- Depositの不明ledger結果: `PullPending → ReconciliationHold → Escrowed`または`Cancelled`
- Withdrawal release: `Observed → ReleasePending → ReleaseTransferred → AcknowledgePending → Released`または`AcknowledgeReverted`
- Withdrawal refund: `Observed → RefundPending → Refunded`または、確定的未送金時の`ReleaseCancellationPending → ReleaseCancelled → RefundPending → Refunded`
- Withdrawalの不明ledger結果: `ReleasePending → ReconciliationHold → ReleaseTransferred`または新しいtransfer identityを持つ`ReleasePending`
- EVM操作: `Queued → Prepared → Submitted → Confirmed`または`Reverted`
- Reconciliation Hold: `Open → ResolvedSucceeded`または`ResolvedAbsent`

同一eventのretryは現在のstateとpayloadが一致するときだけ冪等になる。
同一IDの受付payload hashが異なるretryはconflictとして拒否する。
Deposit IDはdomain-separated hash `(caller, owner_sequence)`で決定する。owner別sequenceは受付の原子的保存に成功した場合だけ増加し、gapは`SequenceMismatch`、同一sequenceの異なるpayloadは`DepositConflict`として外部call前または保存直前の再確認で拒否する。
`Released`と`Refunded`はterminalで相互遷移せず、Release開始後はRefund経路へ入れない。
Deposit Service Feeは受付時の単一safe Base snapshotで固定し、Deposit mintのsafe確認時にだけ会計へ加算する。受付は同snapshotの実効window消費量、stableな未確定Mint予約量、新規net量をchecked加算し、limit超過をLedger pull前に拒否する。safe snapshotは最大60秒だけ再利用し、進捗より古いsnapshotを拒否する。refreshはsingle-flight、失敗後60秒cooldown、放置lockは300秒で失効する。新規の有効な受付試行はstableな全体・caller別quotaを外部call前に消費し、冪等retryと無効入力はquotaを消費しない。予約は`PullPending`から保持し、`Minted`または`Cancelled`でだけ解放する。
Reconciliation Holdはrequest種別・request ID・transfer identity・hold IDが一致する証拠付きresolutionだけを受理する。
成功証拠はledger block index、不存在証拠はLedger全範囲と同期済みIndexの両方で不在を確認したwatermarkを必須とし、request recordとHold recordを一体で遷移・保存する。照合は指定されたHoldまたはFee Payoutだけを処理し、Ledgerを1,000 transaction、Indexを100件単位で増分走査する。明示操作1回の照合callは最大4回で、Ledger tip、archive範囲、Index cursorはstable SQLite stateへ保存する。次stepは別の明示操作を必要とし、失敗やupgrade後も同じ位置から再開する。
Holdから直接transfer、refund、補償へ遷移しない。
Depositの不存在が完全履歴scanで確定した場合はDeposit IDを再利用不能な`Cancelled`へ遷移させる。
Ledgerがallowance不足、残高不足、Bad Feeなど資産移動なしの確定失敗を返した場合も同じIDを`Cancelled`へ終端化し、Mint予約を直ちに解放する。temporarily unavailable、future timestamp、generic errorは`PullPending`を維持して停止し、自動再送しない。再送は次の明示Continueで同じtransfer identityを一回だけ使用する。
Withdrawalの不存在が確定した場合だけ、経済的payloadを維持した新しいtransfer identityとattempt番号でReleaseを再開する。

adapterはcycles残高、signer ETH残高、gas予算からSettlement Reserveを算出する。
残高を観測できない場合またはreserveが不足する場合は、ICRC pull前に新規Depositだけを拒否する。Submitted EVM operationは自動confirmation確認で進み、それ以外の障害停止は明示Retryまで進まない。
nonce未割当のEVM操作は指定されたDepositまたはWithdrawalの1件だけへ割り当てる。各operationは固有のnonce、threshold ECDSA署名、broadcast、Safe-confirmed receiptを持ち、他利用者のoperationとbatchしない。別operationが次nonceを確保したまま停止している場合は`NonceBlocked`で停止し、対象operationの明示Continueを要求する。

pause principalは新規Depositを停止できる。
Governance principalだけがDeposit受付の再開とruntime administrator rotationを実行できる。
finance administratorだけがFee Recipient変更とfee payoutを実行できる。
24時間を超えた`Pending`または`ReconciliationHold`のfee payoutは、権限を持つcallerの`continue_fee_payout`ごとにLedger履歴とIndexを1 stepだけ照合する。成功確認時はfee reserve debitとterminal stateを同じ保存処理で確定し、同一block indexの再実行では二重debitしない。
pause、resume、管理者rotation、Fee Recipient変更、fee payout、reserve gateはappend-only監査ログへ保存する。Base Service Fee表示はcontractの`bridgeSnapshot()`をフロントが直接読むためcanister監査状態へ保存しない。

Withdrawalは認証済みcallerが送るBase transaction hashを同じupdate call内で一度だけ検証する。canisterはsafe receipt内のBridge event、`Releasing`状態、IC owner、Bridge signerを検証して`ReleasePending`へ直接遷移させ、そのcall内でLedger送金を開始する。通知queueは持たない。domain/origin制限はフロントのCSPとwrite UXにだけ使用し、canister認可境界とはみなさない。

terminal EVM operationはtransaction hash、receipt block、Safe確認headを保存する。Mint、cancel、refund、acknowledgementはSafeを2、5、10分後に確認する。10分時点でも未確定なら`ConfirmationCheckExhausted`で失敗として停止する。RPC失敗・不一致・不正応答は即座にscheduleを解除し、自動再試行しない。revertはoperationと所有recordをterminalなReverted状態へ遷移させ、新規Depositを自動pauseする。未解決revertが存在する間はGovernanceによるresumeも拒否し、同一transactionを再送しない。

Ledger、Ledger archive、Index callは15秒、EVM RPC callは30秒、threshold ECDSA public key・署名callは60秒のbounded waitで停止する。timeout後に同一callを自動再試行しない。Ledger transfer timeoutは結果不明としてHoldへ、EVM timeoutは現在stateを維持した停止へ、署名timeoutはenvelopeを`Prepared`のまま維持した停止へ遷移する。

Deposit/Withdrawalの所有者、Governance、pause administratorだけが対応するRetryを実行できる。schedule中は`AutomaticProgressPending`を返し、外部callを行わない。手動Retryはstableな10分windowでglobal 60、caller 6、record 3を既定上限とし、認可・ID・terminal・`Busy`・自動処理中の拒否はquotaを消費しない。Fee payoutは既存のpayout権限と手動フローを維持する。heap上のin-flight guardはrecordごとに独立し、schedulerも同じguardを取得する。

## Stable schema v6

`ic-sqlite-vfs 2.0.0`の単一SQLite DBを`MemoryId(120)`へ保存する。
旧stable-structures用の`MemoryId 0..=32`は廃止済みとして永久に再利用しない。
singleton、domain record、active-state索引、owner索引、counterは同じDB内の`STRICT` tableに置き、BLOB主キーtableは`WITHOUT ROWID`とする。

本番未デプロイのためlegacy migrationは持たず、schema v6だけを受理する。confirmation schedule、手動Retry quota、scheduler healthも同じstable SQLite DBに保持し、`init`と`post_upgrade`で最短scheduleのtimerを復元する。
異なるschema versionはfail closedで拒否する。

各valueは先頭1 byteのwire versionとCBOR payloadからなる最大16 KiBのbounded blobである。
未知version、decode失敗、超過サイズ、未知schema versionはerrorまたはtrapとしてfail closedに扱い、default stateへ置換しない。
stateはSQLiteへrecord単位で直接保存し、`pre_upgrade`で一括serializeしない。
pending/queued/reverted EVM操作、open Hold、pending Ledger操作、nonterminal Withdrawal、予約Mint量、pending fee payout debit、audit sequence、fee payout IDのcounterはsingleton stateへ保存する。
`get_bridge_status`は履歴mapを走査せず、counterとprogress cellから状態を返す。

EVMの実行payloadは`evm_execution_payloads`にoperationごとに1件だけ保存する。
`Queued`は`AwaitingNonce(EvmCallIntent)`、`Prepared`は`Prepared(EvmTransactionEnvelope)`と対応し、nonce割当時はpayload置換、operation遷移、nonce加算を同じSQLite transactionで確定する。
署名済みraw transactionはbroadcast前にenvelopeへ保存し、RPC失敗後の明示Continueでは同じtransactionを再利用する。
`Submitted`以降はpayloadを削除し、`Confirmed`または`Reverted`ではoperation-owner索引も削除する。
terminal operation本体にはtransaction hash、receipt block、Safe確認headを残す。

audit eventは直近10,000件だけを保持する。
10,001件目以降のappendでは最古eventを同じSQLite transaction内で1件削除し、削除件数、削除済み最終sequence、累積SHA-256 digestを更新する。
digest入力はdomain separator、直前digest、sequenceのbig-endian 8 byte、保存wire blob長のbig-endian 8 byte、保存済みwire blobの順である。
`get_audit_events`は保持中event、oldest available sequence、next cursor、pruned件数・範囲・digestを返し、削除済みcursorは保持中の先頭へ補正する。

owner別Deposit履歴索引は新規受付時に最新100件へ刈り込む。
刈り込みは他ownerへ影響せず、古いDeposit本体、Deposit intent、owner sequenceはID照会、同一request retry、payload conflict検出のため保持する。
`list_deposit_ids`は`history_truncated`と`oldest_available_cursor`を返す。
`get_bridge_status.counts`はactive EVM payload、retained/pruned audit event、retained Deposit index entryも返す。
