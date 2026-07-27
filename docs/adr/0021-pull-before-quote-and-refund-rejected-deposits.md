---
status: superseded
---

# Deposit受付とLedger pullをstable executorで分離する

このADRは、無資金intentによるadmission DoSを解消するschema v24のfunding-attempt方式により置き換えられた。

preflight通過はquoteやmint reserveの確約ではない。Canisterは既存schemaへ`FundingPending` record、stable executor job、固定transfer identity、sequence、quotaを単一transactionで保存して即時に返す。Ledger pullはjob leaseを取得したexecutorだけが行う。pull成功またはDuplicateを確定した場合だけ`EscrowedUnquoted`へ昇格し、fresh Finalized Base snapshot、最新counter、reserve tokenに対するpause、Service Fee、Per-Deposit Limit、Mint Throughput Limit、reserveを再検証する。

確定的なLedger失敗は既存`Cancelled`へ進める。結果不明またはcallback消失は同じtransactionで`FundingReconciliationHold`とtransfer identityを保存する。成功証拠またはtip・watermark・連続segmentを含む完全な不存在certificateなしに再送、取消し、補償へ進まない。

quoteとmint予約はpull確定後だけ単一storage transactionで確定する。RPC障害、provider不一致、Bridge signer不一致、stale observationは返金理由にせず、`EscrowedUnquoted`で停止して再観測する。

## Considered Options

- update call内でLedger pullまで行う案は、callback消失時に正式recordとtransfer identityの原子的な正本を失うため不採用とする。
- 正式recordと分離したfunding attempt tableはschema変更と公開履歴の意味論変更を伴うため不採用とする。
- 時間経過だけで曖昧なpullを再送する案は二重pullを生じ得るため不採用とする。

## Consequences

- `FundingPending`は正式Depositのcounter、history、sequence、jobへ含めるが、quote、nonce、mint reserveを持たない。
- 公開`DepositError`、schema v22、wire v18、`FundingPending`の履歴意味論を維持する。
- lease callbackはjob ID、generation、transfer identityのCASを満たす場合だけ状態を更新する。
- timer、manual、confirmationのどの経路もHold証拠要件とlease claimを迂回しない。
- preflight後の競合や状態変化で最終admissionが失敗した場合は既存refund経路へ進み、ユーザーはpullとrefundの両方のLedger feeを負担し得る。
