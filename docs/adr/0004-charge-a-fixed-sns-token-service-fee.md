---
status: accepted
---

# 成功したBridge要求へ固定SNS-token feeを課す

成功した各DepositとWithdrawalへ、送金額に比例しない固定Service FeeをSNSトークンで課す。Bridge原価は主にtransaction件数へ依存するため割合feeを採用しない。現在のService Feeは変更可能とするが、デプロイ時にraw unitで固定したimmutableな`MAX_SERVICE_FEE`を超えられない。

## Considered Options

- 割合feeは大口移動の負担が過大になり、全量移動を許容する目的と衝突するため不採用とする。
- 固定feeと割合feeの併用はfee上限、端数、最小額の規則を増やすため不採用とする。
- 成功した要求ごとの固定feeを採用する。

## Consequences

- DepositのBase mint量は、ICPでロックした量からService Feeを引いた量とする。
- WithdrawalのICP受取量は、Baseでburnした量からService Feeだけを引いた固定`amountOut`とし、Ledger FeeはBridgeが負担する。
- Depositは`max_service_fee`、Withdrawalは`minAmountOut`により、処理中のfee変更から利用者を保護する。
- Service Feeの変更範囲は`0 <= service_fee <= MAX_SERVICE_FEE`とし、`MAX_SERVICE_FEE`自体は変更不能にする。
- 上限を超えるfee変更はBase contractとBridge canisterの双方で拒否する。
- Service FeeはBase mintまたはICP Releaseの成功時にのみfee reserveへ確定する。
- fee reserveはBridge Exposureの裏付けと分離して会計する。
- 管理者はFee Recipientを変更できる。変更はeventと監査ログへ記録する。
- Fee Recipient変更時、未送金の確定済みfee reserve全体を新recipientへ帰属させる。
- recipient別fee bucketや旧recipient向け残高を保持しない。
- fee送金は確定済みfee reserveだけを対象とし、Bridge Exposureの裏付け資産を送金できない。
- Fee Recipientの変更権限とService Feeの変更権限は、mint、refund、任意送金の権限を含まない。
- SNS-token feeはBase gas用ETHへ自動変換しない。運用者がETHを補充する手順を別途持つ。
- VerusとSolidity SMTCheckerで、Service Feeの上限制約、二重計上防止、成功前のfee確定禁止、recipient変更時のreserve保存、fee reserveを超える送金の禁止を証明する。
