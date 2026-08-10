# Bridge canister状態機械

## 永続化と実行境界

`bridge-core`はcaller、時刻、ICRC Ledger、EVM RPC、Candid、storageに依存しない決定的な状態遷移を定義する。`bridge-canister`は単一SQLite DBへ状態を保存し、Ledger、EVM RPC、threshold ECDSA、管理API、stable job executorを接続する。

通常の再オープンと`post_upgrade`はstable schema v32、record wire version v28だけを受理する。v31以下、未知schema、未知wire、decode不能なDBはfail closedで起動を拒否する。
upgrade検証はcurrent schema v32のrecord・config・quota・auditを保持するsame-Wasm再オープンと、旧schema・wireの拒否を検証する。

`settlement_jobs`が実行中・停止中Settlementの正本である。Depositとfee payoutはtimerが自動claimし、Withdrawalは明示的な`continue_withdrawal`だけがmanual claimする。Withdrawal通知時はrecordと固定transfer identityだけをatomic保存し、jobを作らない。外部`await`前に署名dispatchやLedger transfer identityを永続化し、lease generationとDB上の状態だけが実行権を決める。

Mint用Base transaction laneは存在しない。Governance laneはnonce、署名済みgeneration、raw transactionとhashだけを永続化する。Canisterはbroadcast、receipt監視、rebroadcast、自動replacementを行わず、外部relayerが送信後に指定hashのFinalized結果をCanisterへ通知する。

## Deposit（ICP → Base）

Deposit IDはdomain-separated hashの`(canister ID, Base chain ID, Bridge address, deployment instance ID, caller, owner_sequence)`で決まり、同じinstall domainとsequenceの異なるpayloadは`DepositConflict`になる。受付時は有料Base preflightより先に、正式Depositとは別の`Prepared` funding attempt、固定transfer identity、消費済みdeposit quota、active funding reservationを保存し、cycle reserveを確認する。このadmission成功後だけ、同じcanonical Base snapshotで候補IDが未処理であることを確認し、attemptを`Dispatched`へ進めて同じupdate callでICRC-2 pullを行う。Base preflightまたはLedgerの確定的失敗ではattemptとactive reservationを削除するが、開始済みpreflightのquotaは戻さない。

```text
FundingAttempt
  ├─ Ledger成功 / Duplicate → EscrowedUnquoted
  ├─ Base/Ledger確定的失敗 → attempt削除（正式Depositなし、quotaは保持）
  └─ Ledger結果不明        → FundingReconciliationHold
                                ├─ 成功証拠       → EscrowedUnquoted
                                └─ 完全な不存在証拠 → Cancelled

EscrowedUnquoted
  ├─ Finalized quote・capacity予約 → AuthorizationPending
  ├─ pause・fee・limit拒否         → RefundPending → Refunded
  └─ RPC障害・観測不一致           → 停止（返金しない）

AuthorizationPending
  ├─ 同一digestへのthreshold ECDSA署名 → AuthorizationAvailable
  └─ 認可発行前の確定失敗               → RefundAvailable

AuthorizationAvailable
  └─ Finalized timestampがdeadline超過
       → RefundAvailable（予約解放、Base未照合）

RefundAvailable
  └─ owner claim（Base outcallなし） → RefundPending → Refunded

RefundAvailable
  └─ owner claim
       ├─ exact Mint証拠        → Minted
       ├─ Finalized未処理証拠  → RefundPending → Refunded
       └─ 不一致・証拠欠落     → fail closed（資金移動なし）

RefundPending
  └─ Ledger結果不明 → RefundReconciliationHold
                         └─ 非anonymous callerの再請求で同一transferを照合
```

1. `EscrowedUnquoted → AuthorizationPending`では、Finalized Base snapshot、quote、全Authorization field、EIP-712 domain、digest、作成元Finalized block number/hash/timestamp、mint capacity予約、jobを一つのSQLite transactionで保存する。
2. deadlineは作成元Finalized Base timestampへ固定TTL 2時間（7,200秒）をchecked-addして一度だけ決める。IC時刻、ブラウザ時刻、再試行時刻から作らない。
3. threshold ECDSAの`await`前にdispatch済みフラグとattempt番号を保存する。timeout、callback消失、upgrade後も同一digestだけを再署名し、deadlineやpayloadを変更しない。65-byte署名はlow-s `r || s || v`へ正規化し、復元addressが期待するMint Signerと一致した場合だけ、同じtransactionでservice feeを一度だけfee reserveへ計上して公開する。
4. `AuthorizationAvailable`では任意Base walletが署名済みpayloadをcontractへ送り、そのwalletがgasを支払う。Canisterは期間中のtransactionやreceiptを追跡しない。
5. 新規Depositなどで既に取得したBase Finalized snapshotを使い、deadline順indexをcall単位の上限までローカル走査する。`finalized_timestamp > deadline`だけを期限切れとし、等値では予約を保持する。`AuthorizationPending`は`RefundAvailable`、`AuthorizationAvailable`は`RefundAvailable`へ進め、個別`isDepositProcessed`照合は行わない。
6. backlogが残る場合は未処理予約を保守的に過大計上する。新規受付上限を正確に判定できなければretry可能エラーにし、過少計上しない。新規Depositがなければ予約枠も消費されないため、期限処理timerは設けない。
7. 任意の非anonymous Principalが`request_deposit_refund`を呼ぶとRefundを進める。宛先、金額、transfer identityは既存recordに固定され、caller入力を受けない。認可発行前の`RefundAvailable`はBase outcallなしで`gross - refund ledger fee`を送る。最初のICRC-2 pull feeはWallet負担のまま戻さない。
8. 認可発行済みの`RefundAvailable`では、同じcanonical Finalized block hashへruntime identity、signer、epoch、strict deadline、`isDepositProcessed(depositId)`をEIP-1898で束縛する。`processed == false`だけを`gross - charged service fee - refund ledger fee`で返金する。service fee、初回pull fee、refund feeは返さない。
9. `processed == true`なら、作成元blockから観測Finalized headまでの`DepositMinted`を取得し、件数1、contract、digest、recipient、amount、fee、canonical成功receiptを検証して`Minted`へ進む。event欠落・複数・内容不一致、RPC不一致、Finalized停止、runtime不一致では資金を動かさない。
10. Ledger結果不明は同一transfer identityを`RefundReconciliationHold`に保持する。timer retryは行わず、任意の非anonymous callerの再請求で照合を1 step進める。Duplicateは同一送金の成功として扱い、完全な不存在証拠なしに別identityを発行しない。
11. pause、epoch変更、signer rotationは未期限AuthorizationをContract上で失効させるが、早期返金の根拠にはしない。元のdeadlineとFinalized未処理証拠を必ず通す。

未処理Authorizationはdeadline超過を観測するまでmint window liabilityとして予約する。Deposit admissionはMint Signer ETH、gas price、nonceへ依存しない。cycles floorとsettlement cycle ceilingは署名、明示Refund時のRPC・Ledger処理のため維持する。

## Withdrawal（Base → ICP）

WithdrawalはBase walletが`createWithdrawal`を送り、同一transactionでbSNSの`transferFrom`、burn、固定受取額を持つ`Committed` recordを作る。Canisterはtransactionを生成しない。

```text
Base Committed
  → notify_withdrawal(transaction_hash)
  → canonical Finalized receipt・event・state・snapshot検証
  → Observed → ReleasePending（自動jobなし）
  → continue_withdrawal（1 call 1 external step）
       ├─ 成功 → Paid
       └─ ReconciliationHold
            ├─ 成功証拠 → Paid
            └─ 完全な不存在証拠 → 新identityのReleasePending（送金は次回）
```

UIはtransaction hashをlocalStorageへ保存し、Finalized eventを検出した後にdeployment-scopedなbrowser identityから`notify_withdrawal`を呼び、成功後に同じidentityで`continue_withdrawal`を1回だけ呼ぶ。通知・継続にIC walletの署名やICRC-21同意取得は使用しない。非終端ならHistoryの明示操作で再開する。Canisterはreceipt、event、`getWithdrawal`、Bridge snapshotを同じcanonical Finalized block hashへ束縛する。Ledger結果不明は時間経過だけで失敗扱いにせず、LedgerとIndexの完全なwatermarkで不存在を証明できるまでHoldを維持する。

## 公開APIと権限

| API | 呼び出し元 | 役割 |
|---|---|---|
| `request_deposit` | Deposit owner | Ledger pullとAuthorization作成開始 |
| `request_deposit_refund` | 任意の非anonymous Principal | claimable amount確認、必要なFinalized照合、固定Ledger refundまたはhold再照合 |
| `notify_withdrawal` | 任意の非anonymous Principal | Finalized Withdrawalのpermissionless通知。送金先はBase eventへ束縛 |
| `continue_withdrawal` | 任意の非anonymous Principal | 固定内容のLedger releaseまたは照合を最大1 external step進める |
| Base governance prepare/status/replace/confirm | Governance、またはpause/cancelに限りpause principal | 外部relayer向け署名成果物とFinalized確定 |
| `prepare_next_emergency_base_action` | Governance、pause principal | emergency queueのpause/cancelを順に署名 |
| `get_deposit` / `get_deposit_by_owner_sequence` | 公開query | Authorization、deadline、signature、状態を照会 |
| `get_bridge_status` | 公開query | Finalized観測、epoch、Governance reserve、schedulerを照会 |

SNS Governance principalはresume、principal rotation、Fee Recipient、fee payout、Service Fee、Timelock操作を行う。pause principalは緊急pauseと許可された進行だけを行う。Mint SignerはEIP-712 Authorization専用、Governance OperatorはCanister発Base governance transaction専用で、derivation pathとETH管理を分離する。
