---
status: accepted
---

# Ledger pull後にDepositをquoteし、拒否時は固定feeで返金する

Deposit受付ではBase quote、mint nonce、mint reserveを予約せず、`FundingPending`とLedger pull jobだけを保存する。pull成功またはDuplicateを確定した後に`EscrowedUnquoted`へ進み、同一のFinalized Base snapshotに対するpause、Service Fee、Per-Deposit Limit、Mint Throughput Limit、reserveを再検証する。

検証に成功した場合だけquote、MintDeposit operation、mint予約を単一storage transactionで確定する。freshな観測で拒否を確定した場合は、元のIC accountへ`gross_amount - KINIC_LEDGER_FEE`を返し、escrowから`KINIC_LEDGER_FEE`を支払う。返金ではService Feeを確定せず、escrow debit合計をgross amountに固定する。定数の正本は`canister/bridge-canister/src/ledger.rs`とする。

RPC障害、provider不一致、Bridge signer不一致、stale observationは返金理由にせず、`EscrowedUnquoted`で停止して再観測する。返金結果が不明なら`RefundReconciliationHold`へ移し、成功証拠または完全な不存在証明なしに新attemptを作らない。不存在証明後のattemptは番号、created-at time、memoだけを変更し、from、to、amount、feeを維持する。

## Considered Options

- Base quoteをpull前に固定する案は、pullまでに変化したpause、limit、reserveを安全に扱えず不採用とする。
- pull後の拒否を無期限holdにする案は資産安全を保つが、確定的な業務拒否でも利用者資産を拘束するため不採用とする。
- refund feeを動的に追従する案は経済payloadを変化させるため不採用とし、デプロイ固定値を使う。

## Consequences

- `gross_amount <= KINIC_LEDGER_FEE`はrecord作成、Ledger call、owner sequence消費より前に拒否する。
- 未quote状態ではService Feeとnet amountを0値で表現せず、quoteを保持しない。
- `FundingPending`と`RefundPending`はLedger pending counterへ含めるが、mint予約はquote済みmint状態だけを集計する。
- `request_deposit`、Withdrawal notification、mint-revert recoveryはjob保存後に即時returnし、timerまたは明示Continueが進行する。
- ADR 0006の「Deposit返金を禁止する」決定を、このADRの確定的post-pull拒否と専用refund reconciliationに限って置換する。曖昧なfunding結果を補償しない原則は維持する。
