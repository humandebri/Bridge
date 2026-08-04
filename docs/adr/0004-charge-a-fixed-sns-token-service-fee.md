---
status: accepted
---

# 確定したBridge要求へ固定SNS-token feeを課す

各DepositとWithdrawalへ、送金額に比例しない固定Service FeeをSNSトークンで課す。Depositのfeeは署名済みMint Authorizationを永続化した時点、WithdrawalのfeeはBase上でWithdrawalが成立した時点で確定する。Bridge原価は主に要求処理件数へ依存するため割合feeを採用しない。現在のService Feeは変更可能とするが、デプロイ時にraw unitで固定したimmutableな`MAX_SERVICE_FEE`を超えられない。

この改訂は、旧版の「Deposit feeはBase mint成功時だけ確定する」という記述を置き換える。wallet送信型Mint Authorizationでは、Canisterが署名を発行した時点で署名・RPC・保存処理を完了し、利用者が期限内にmintを送信できる能力を提供しているためである。

## Considered Options

- 割合feeは大口移動の負担が過大になり、全量移動を許容する目的と衝突するため不採用とする。
- 固定feeと割合feeの併用はfee上限、端数、最小額の規則を増やすため不採用とする。
- 上記の確定境界へ到達した要求ごとの固定feeを採用する。

## Consequences

- DepositのBase mint量は、ICPでロックした量からService Feeを引いた量とする。
- WithdrawalのICP受取量は、Baseでburnした量からService Feeだけを引いた固定`amountOut`とし、Ledger FeeはBridgeが負担する。
- Depositは`max_service_fee`、Withdrawalは`maxServiceFee`により、処理中のfee変更から利用者を保護する。Withdrawalの`amountOut`は実行時に`amount - chargedServiceFee`として固定される。
- Service Feeの変更範囲は`0 <= service_fee <= MAX_SERVICE_FEE`とし、`MAX_SERVICE_FEE`自体は変更不能にする。
- 上限を超えるfee変更はBase contractとBridge canisterの双方で拒否する。
- Deposit Service Feeは署名済みMint Authorizationの保存と同じSQLite transactionで一度だけfee reserveへ確定する。署名前の失敗では確定しない。
- 発行済みAuthorizationが未送信、revert、期限切れ、または期限後に未処理証拠を伴って返金された場合も、確定済みDeposit Service Feeは返却しない。
- Withdrawal Service Feeは従来どおりBase Withdrawalの成立時に確定し、ICP Releaseの再試行では二重計上しない。
- fee reserveはBridge Exposureの裏付けと分離して会計する。
- 管理者はFee Recipientを変更できる。変更はeventと監査ログへ記録する。
- Fee Recipient変更時、未送金の確定済みfee reserve全体を新recipientへ帰属させる。
- recipient別fee bucketや旧recipient向け残高を保持しない。
- fee送金は確定済みfee reserveだけを対象とし、Bridge Exposureの裏付け資産を送金できない。
- Fee Recipientの変更権限とService Feeの変更権限は、mint、Withdrawal再mint、任意送金の権限を含まない。
- SNS-token feeはBase gas用ETHへ自動変換しない。運用者がETHを補充する手順を別途持つ。
- Verus、Lean、Solidity SMTCheckerとtransaction testで、Service Feeの上限制約、Authorization署名前のDeposit fee確定禁止、確定境界ごとの二重計上防止、recipient変更時のreserve保存、fee reserveを超える送金の禁止を検証する。
