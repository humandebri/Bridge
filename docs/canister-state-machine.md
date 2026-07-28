# Bridge canister状態機械

## 永続化と実行境界

`bridge-core`はcaller、時刻、ICRC Ledger、EVM RPC、Candid、storageに依存しない決定的な状態遷移を定義する。`bridge-canister`は単一SQLite DBへ状態を保存し、Ledger、EVM RPC、threshold ECDSA、管理API、stable job executorを接続する。

stable schema v25、record wire version v21だけを受理する。本番未デプロイのためmigration、dual-read、fallbackは持たず、旧・未知schema、旧wire version、decode不能なDBはfail closedで起動を拒否する。
upgrade検証はcurrent schema v25の再オープンだけを成功経路とし、それ以前のschemaを変換しない。

`settlement_jobs`が自動・手動進行の正本である。recordとjobは同じSQLite transactionで更新し、外部`await`前に署名dispatchやLedger transfer identityを永続化する。timerは目覚ましにすぎず、lease generationとDB上の状態だけが実行権を決める。

Mint用Base transaction laneは存在しない。nonce、raw transaction、broadcast、rebroadcast、replacement、receipt confirmationはGovernance Operatorのtransaction laneだけに限定する。

## Deposit（ICP → Base）

Deposit IDはdomain-separated hashの`(caller, owner_sequence)`で決まり、同じsequenceの異なるpayloadは`DepositConflict`になる。受付時は正式Depositとは別のfunding attempt、固定transfer identity、quota reservationだけを保存し、同じupdate callでICRC-2 pullを行う。

```text
FundingAttempt
  ├─ Ledger成功 / Duplicate → EscrowedUnquoted
  ├─ Ledger確定的失敗      → attempt削除（正式Depositなし）
  └─ Ledger結果不明        → FundingReconciliationHold
                                ├─ 成功証拠       → EscrowedUnquoted
                                └─ 完全な不存在証拠 → Cancelled

EscrowedUnquoted
  ├─ Finalized quote・capacity予約 → AuthorizationPending
  ├─ pause・fee・limit拒否         → RefundPending → Refunded
  └─ RPC障害・観測不一致           → 停止（返金しない）

AuthorizationPending
  ├─ 同一digestへのthreshold ECDSA署名 → AuthorizationAvailable
  └─ 署名不能のままFinalized期限超過     → ExpiryReconciliation

AuthorizationAvailable
  └─ 期限到達後 → ExpiryReconciliation
                    ├─ exact Mint証拠   → Minted
                    ├─ Finalized未処理証拠 → RefundPending → Refunded
                    └─ 不一致・証拠欠落 → 停止（返金しない）
```

1. `EscrowedUnquoted → AuthorizationPending`では、Finalized Base snapshot、quote、全Authorization field、EIP-712 domain、digest、作成元Finalized block number/hash/timestamp、mint capacity予約、jobを一つのSQLite transactionで保存する。
2. deadlineは作成元Finalized Base timestampへ固定TTL 2時間（7,200秒）をchecked-addして一度だけ決める。IC時刻、ブラウザ時刻、再試行時刻から作らない。
3. threshold ECDSAの`await`前にdispatch済みフラグとattempt番号を保存する。timeout、callback消失、upgrade後も同一digestだけを再署名し、deadlineやpayloadを変更しない。65-byte署名はlow-s `r || s || v`へ正規化し、復元addressが期待するMint Signerと一致した場合だけ公開する。
4. `AuthorizationAvailable`では任意Base walletが署名済みpayloadをcontractへ送り、そのwalletがgasを支払う。Canisterは期間中のtransactionやreceiptを追跡しない。
5. jobはおおよそのIC時刻で起床できるが、安全判定にはBase Finalized timestampだけを使う。`finalized_timestamp <= deadline`なら60秒後へ延期する。`timestamp == deadline`はContractでMint可能なため返金不可である。
6. 期限後は同じFinalized block hashへruntime identity、signer、epoch、timestamp、`isDepositProcessed(depositId)`をEIP-1898で束縛する。RPC不一致、Finalized停止、runtime不一致では返金せず再試行する。
7. `processed == false`なら、Authorization digest、chain、Finalized block、timestamp、runtime、RPC request/response digestを失効証拠として保存した同じtransactionで`RefundPending`へ進む。その後だけLedger refundを実行する。
8. `processed == true`なら、作成元blockから観測Finalized headまでの`DepositMinted`を取得し、件数1、contract、digest、recipient、amount、fee、canonical成功receiptを検証する。transaction/receipt block/Finalized head/RPC digestをMint証拠として保存した同じtransactionで`Minted`へ進む。
9. processedなのにeventがない、複数ある、内容が異なる場合は新規Depositをlocal pauseし、対象Depositを停止する。返金へfallbackしない。
10. pause、epoch変更、signer rotationは未期限AuthorizationをContract上で失効させるが、早期返金の根拠にはしない。元のdeadlineとFinalized未処理証拠を必ず通す。

`continue_deposit`はowner、Governance、pause principalが停止jobを再開するAPIである。署名が失敗した`AuthorizationPending`も、Base Finalized timestampがdeadlineを超えていれば署名を待たず`ExpiryReconciliation`へ進める。`AuthorizationAvailable`または`ExpiryReconciliation`を含め、手動起動でも上記Finalized条件を迂回できない。

未処理Authorizationはterminal状態までmint window liabilityとして予約する。Deposit admissionはMint Signer ETH、gas price、nonceへ依存しない。cycles floorとsettlement cycle ceilingは署名、RPC、Ledger処理のため維持する。

## Withdrawal（Base → ICP）

WithdrawalはBase walletが`createWithdrawal`を送り、同一transactionでbSNSの`transferFrom`、burn、固定受取額を持つ`Committed` recordを作る。Canisterはtransactionを生成しない。

```text
Base Committed
  → notify_withdrawal(transaction_hash)
  → canonical Finalized receipt・event・state・snapshot検証
  → Observed → ReleasePending → Paid
                            └→ ReconciliationHold
                                  ├─ 成功証拠 → Paid
                                  └─ 完全な不存在証拠 → ReleasePending
```

UIはtransaction hashをlocalStorageへ保存し、Finalized eventを検出した後にIC walletから`notify_withdrawal`を呼ぶ。Canisterはreceipt、event、`getWithdrawal`、Bridge snapshotを同じcanonical Finalized block hashへ束縛する。Ledger結果不明は時間経過だけで失敗扱いにせず、LedgerとIndexの完全なwatermarkで不存在を証明できるまでHoldを維持する。

## 公開APIと権限

| API | 呼び出し元 | 役割 |
|---|---|---|
| `request_deposit` | Deposit owner | Ledger pullとAuthorization作成開始 |
| `continue_deposit` | owner、Governance、pause principal | 署名・期限照合・返金の停止後再開 |
| `notify_withdrawal` | Withdrawal owner、Governance、pause principal | Finalized Withdrawalの通知 |
| `continue_withdrawal` | owner、Governance、pause principal | Ledger release・照合の再開 |
| `get_deposit` / `get_deposit_by_owner_sequence` | 公開query | Authorization、deadline、signature、状態を照会 |
| `get_bridge_status` | 公開query | Finalized観測、epoch、Governance reserve、schedulerを照会 |

SNS Governance principalはresume、principal rotation、Fee Recipient、fee payout、Service Fee、Timelock操作を行う。pause principalは緊急pauseと許可された進行だけを行う。Mint SignerはEIP-712 Authorization専用、Governance OperatorはCanister発Base governance transaction専用で、derivation pathとETH管理を分離する。
