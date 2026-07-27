# Bridge canister状態機械

## 前提

`bridge-core`はcaller、時刻、ICRC Ledger、EVM RPC、Candid、stable storageに依存しない決定的な状態遷移を定義する。`bridge-canister`はその状態を単一SQLite DBへ保存し、ICRC Ledger、EVM RPC、threshold ECDSA、管理API、stable job executorを接続する。

Stable schema v24、record wire version v20だけを受理し、status APIもschema v24を返す。本番未デプロイのためmigration、dual-read、fallbackは持たない。旧schema、未知schema、旧wire version、decode不能なDBはfail closedで起動を拒否し、開発・staging Canisterはreinstallする。

install時の不変なruntime設定は`singleton_state.config`、rotation可能なGovernance principal、Pause principal、Fee Recipientは`singleton_state.admin_state`だけを永続的な正本とする。`get_public_config()`は両者を合成し、管理状態には常にrotation後の現在値を返す。

`settlement_jobs`がSettlement実行の永続的な正本である。recordと`phase = settlement`のjobは同じSQLite transactionで作成され、init・post-upgrade・job更新後はDBに保存された最短起床時刻からone-shot timerを再登録する。timerは実行の目覚ましであり、timer ID自体に状態上の意味はない。

EVM transactionが`Submitted`になったときだけjobは`phase = confirmation`へ移り、`AwaitingConfirmation`として起床時刻なしで待機する。これはDepositの`MintDeposit`にだけ存在し、`confirm_deposit`が受け取ったtransaction hash・receipt block・観測Finalized blockをclaimする。Canister timerはEVM confirmationのfallbackを行わない。confirmation後はjobをSettlement phaseへ戻し、Ledger送金などの後続処理をtimerで進める。

## Deposit（ICP → Base）

Deposit IDはdomain-separated hashの`(caller, owner_sequence)`で決まり、同じsequenceの異なるpayloadは`DepositConflict`になる。受付時は正式Depositとは別のfunding attempt、固定transfer identity、quota reservationだけを保存し、同じupdate callでICRC-2 pullを行う。確定失敗では正式record、history、sequence、job、committed quotaを残さない。Success／Duplicateは`EscrowedUnquoted`へ、結果不明は`FundingReconciliationHold`へ原子的に昇格する。

```text
FundingAttempt
  ├─ Ledger成功 / Duplicate ─→ EscrowedUnquoted（正式Deposit作成）
  ├─ Ledger確定的失敗 ──────→ attempt削除（正式Depositなし）
  └─ Ledger結果不明 ────────→ FundingReconciliationHold（正式Deposit作成）
                                  ├─ 成功証拠 ─→ EscrowedUnquoted
                                  └─ 完全な不存在証拠 ─→ Cancelled

EscrowedUnquoted
  ├─ Finalized Base quote・mint予約成功 ─→ MintPending
  ├─ pause・fee・limit・reserve拒否 ─────→ RefundPending ─→ Refunded
  └─ RPC障害・観測不一致・stale ───────→ EscrowedUnquotedで停止

RefundPending ── Ledger結果不明 ─→ RefundReconciliationHold
                                      ├─ 成功証拠 ─→ Refunded
                                      └─ 完全な不存在証拠 ─→ payload固定の新attempt

MintPending ── Finalized receipt成功 ─→ Minted
            └─ Finalized receipt revert ─→ MintReverted

MintReverted ── Governanceのrecover_mint_revert ─→ MintPending（replacement operation）
```

1. UIはBase chain、Bridge runtime、CanisterのFinalized observation、現在のService Fee、ICRC Ledger残高・fee・allowanceを再検証する。allowanceが不足する場合は、gross amountとLedger feeを含む必要量をICRC-2 approveする。
2. IC walletから`request_deposit`を呼ぶ。Canisterは内部funding attemptを予約し、同じcallでICRC-2 pullを行う。
3. 成功またはDuplicateなら`EscrowedUnquoted`へ原子的に昇格し、確定的失敗はattemptとreservationを削除する。結果不明は`FundingReconciliationHold`へ移し、callback消失は専用recovery scanが同一transfer identityを照合する。成功証拠または完全な不存在証明なしに別identityを送らない。
4. `EscrowedUnquoted`でFinalized Base snapshot、local mint counter、reserve observation tokenを再検証する。成功時だけoptional quote、EVM operation、mint予約を原子的に保存する。freshな拒否は固定payloadのrefundへ進め、一時障害や不一致では返金しない。
5. `MintPending` operationは`Queued → Prepared → Submitted`の順に進む。broadcast後のtransaction hashはcanonical recordとconfirmation jobへ保存され、UIはHistoryから取得する。
6. receipt blockがFinalized headへ到達したら、認証済みIC walletから`confirm_deposit`を呼ぶ。Canisterはsettlement IDと保存済みtransaction hash、receipt block、観測Finalized blockを照合し、EVM RPCのquorumでcanonical Finalized receiptを再検証する。
7. 成功ならoperationを`Confirmed`、Depositを`Minted`へ遷移させる。receiptがrevertならoperationを`Reverted`、Depositを`MintReverted`へ遷移させ、新規Depositをpauseする。reverted transactionは自動再送せず、GovernanceだけがFinalized状態と`isDepositProcessed`を再確認したうえで`recover_mint_revert`を実行できる。
8. confirmation後のSettlementはCanister timerが自動進行するため、ブラウザを閉じてもLedger側の後続処理がある場合は継続する。RPC、署名、nonce、reserveなどで停止したjobは自動再試行せず、原因解消後に所有者またはGovernance・pause principalが`continue_deposit`を呼ぶ。

## Withdrawal（Base → ICP）

WithdrawalはBase上のburnを先に確定させ、その後にCanisterがICP側の固定債務を履行する。Base側の`Committed`は終端状態であり、Base refund、release acknowledgement、cancel、Withdrawal用のthreshold ECDSA transactionは存在しない。

```text
Base wallet
  └─ approve(amount) + createWithdrawal(amount, maxServiceFee, owner, subaccount)
       └─ transferFrom + Bridge残高burn + Committed record + WithdrawalCommitted
            └─ Finalized eventをUIが検出
                 └─ IC wallet → notify_withdrawal(transaction_hash)
                      └─ canonical Finalized検証
                           └─ Observed → ReleasePending → Paid
                                           └─ ReconciliationHold
                                                ├─ 成功証拠 ─→ Paid
                                                └─ 完全な不存在証拠 ─→ ReleasePending
```

1. UIはBase wallet、送付先IC wallet、Base snapshotのService Fee、bSNS残高、chain/runtimeを直前に再検証する。必要ならbSNSのBridge allowanceを要求額ちょうどに設定する。
2. Base walletから`createWithdrawal`を送る。Contractは実行時Service Feeが`maxServiceFee`以下で、`amount > serviceFee`であることを確認し、同じtransaction内で`transferFrom`、Bridge残高のburn、固定quoteを持つ`Committed` record作成、`WithdrawalCommitted`発行を原子的に行う。

   ```text
   chargedServiceFee = 実行時のserviceFee
   amountOut = amount - chargedServiceFee
   ```

3. UIはtransaction hashと送付先IC ownerをlocalStorageのpending confirmationへ保存する。これは追加のBase transactionを予約するものではなく、通知を再開するための公開transaction参照である。秘密鍵や署名情報は保存しない。
4. UIまたはWithdrawal HistoryがFinalized blockのreceiptと`WithdrawalCommitted` eventを検出し、IC walletから`notify_withdrawal`を呼ぶ。Canisterはtransaction hashを起点に、receipt、event、`getWithdrawal`、Bridge snapshotを同じcanonical Finalized block hashへ束縛してquorum検証する。通知を行えるのは対象IC owner、Governance principal、pause principalである。
5. 検証成功後、Ledger feeがcharged Service Fee以下ならCanisterは同じtransaction payloadの`ReleasePending` recordとSettlement jobを原子的に保存する。fee超過時はreleaseを作らず`Observed` record、停止理由、runtime guard、監査eventを保存する。重複通知は既存recordを返し、新しいjobを起動しない。
6. Settlement runnerは固定`amountOut`を固定IC Accountへ送る。Ledger成功、Duplicate、または履歴照合による成功確認で`Paid`になり、`chargedServiceFee - actualLedgerFee`だけをfee reserveへ一度加算する。
7. Ledgerの結果不明は`ReconciliationHold`へ移す。dedup期間内は同一transfer identityで確認し、期間後はLedger全範囲とIndexの同期済みwatermarkで不存在を証明できた場合だけ、同じ宛先・金額を保った新しいtransfer identityで`ReleasePending`へ戻す。Deposit refundの確定的な`BadFee`は固定fee設定の不一致として停止し、amount、fee、transfer identityを変更しない。
8. Base側のpauseは新規Withdrawal作成を止めるが、すでに`Committed`となったICP債務の送金・照合は止めない。停止したSettlementは原因解消後に所有者またはGovernance・pause principalが`continue_withdrawal`を呼ぶ。

## 外部確認の境界

- **Finalizedが唯一のBase確認境界**：Safe head、一定confirmation数、単一RPCの結果へfallbackしない。Finalized headまたはcanonical hashがquorumで収束しない場合はfail closedする。
- **ブラウザの役割**：Depositの`confirm_deposit`、Withdrawalの`notify_withdrawal`をユーザーのIC wallet consent付きで開始する。ブラウザの観測値はCanisterの保存値・EVM RPC quorum検証を置き換えない。
- **Canister timerの役割**：`phase = settlement`のjobを自動実行する。Depositの`phase = confirmation`は起床時刻を持たず、wallet同意付き`confirm_deposit`だけがclaimしてreceiptによるterminal遷移を開始する。Base governance laneのCanister管理transactionは別のliveness policyに従い、Missing時の同一raw再送と上限付きreplacementを行う。
- **Ledger照合**：Ledger transferの不明結果を時間経過だけで失敗扱いにせず、Reconciliation Holdを無期限に保持する。不存在証拠はLedgerとIndexの完全性確認を伴うwatermarkだけである。

## 主要APIと権限

| API | 主な呼び出し元 | 役割 |
|---|---|---|
| `request_deposit` | Deposit ownerのIC wallet | Deposit受付、Ledger pullとBase mint Settlementの開始 |
| `confirm_deposit` | Deposit owner、Governance、pause principal | 保存済みMint transactionのFinalized確認を開始 |
| `continue_deposit` | Deposit owner、Governance、pause principal | 停止したDeposit Settlementを再開 |
| `notify_withdrawal` | WithdrawalのIC owner、Governance、pause principal | Finalized `WithdrawalCommitted`をCanisterへ通知 |
| `continue_withdrawal` | Withdrawal owner、Governance、pause principal | ICP releaseまたはReconciliationを再開 |
| `recover_mint_revert` | Governance principalのみ | Finalized revert済みDepositのreplacement mintを開始 |
| `get_deposit` / `get_deposit_by_owner_sequence` / `get_withdrawal` | 公開query | canonical intent、phase、quote、停止理由、automatic progressを照会 |
| `get_bridge_status` | 公開query | Finalized観測、reserve、scheduler、未決済Withdrawal、revertを照会 |

管理APIの権限はCanister内で分離する。SNS Governance principalだけがresume、pause principal rotation、Fee Recipient、fee payout、Service Fee、Timelock schedule/executeを行う。単一pause principalはIC/Baseのpause、記録済みpending Timelock operationのcancel、許可されたSettlement進行だけを行う。Base側ではCanisterが別々のthreshold derivation pathからMint SignerとGovernance Operatorを導出し、前者をmint専用、後者をpause、Service Fee、Timelock propose/cancel/execute専用とする。人間のfinance principal、release approver、EVM管理walletは置かない。
