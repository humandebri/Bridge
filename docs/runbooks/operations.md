# Bridge資源補充・緊急停止

## 日常確認

- Bridgeとpause-watchdogのcycles、signer ETH、reserve surplus、finalized観測時刻、pending nonceを確認する。
- pause-watchdogは60秒間隔で確認し、reserve不足、15分超の観測停滞、3回連続status失敗で新規Depositだけをpauseする。
- Fee Recipient、RPC credential、raw transaction、秘密情報を監視ログへ出さない。

## ETH・cycles補充

- ETHはprofileのthreshold signer addressへ、Settlement Reserveを上回るまで運用者が送る。SNS-token feeの自動交換は行わない。
- cyclesはBridgeとwatchdogを別々に補充し、30日floorとfreezing thresholdの両方を満たすことを確認する。
- 補充後も自動resumeしない。Governanceが観測回復と資産状態を確認してからBridgeをresumeする。

## 緊急pause

- hardware pause principalまたはwatchdogから`pause_new_deposits`を実行する。既存Settlement、Hold照合、receipt確認は止めない。
- Base側の異常ではRuntime AdministratorがDeposit/Withdrawal pauseを実行する。unpauseとrole rotationはBase Admin walletからTimelockを経由し、limitは変更しない。
- Holdの強制解除、nonce操作、任意transaction送信は行わない。
## Finalized EVM revert

finalized receiptがrevertを示した場合、Bridgeは対象operationとDeposit/Withdrawalを`Reverted`へ終端化し、新規Depositを自動pauseする。監査ログのoperation ID、kind、transaction hash、finalized blockを保存する。未解決revertが1件でもある間は`resume_new_deposits`は`UnresolvedEvmRevert`を返す。

reverted transactionは自動再送も管理APIによるretryも行わない。既存Withdrawal settlement、Reconciliation Hold照合、receipt確認は継続する。復旧では原因を調査し、状態を明示的に扱うcanister upgradeを準備する。schemaやcounterを手作業で書き換えない。
