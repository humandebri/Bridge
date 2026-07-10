---
status: accepted
---

# 単一資源プール内でSettlement Reserveを論理予約する

BridgeはEVM signer、nonce queue、ETH残高、canisterを用途別に分離せず、それぞれ単一とする。既存Withdrawal Settlementを新規Depositより優先するため、ETHとcyclesの一部をSettlement Reserveとして会計上予約する。

## Considered Options

- DepositとSettlementでaddress、role、nonce queue、canisterを物理分離する案は、資金移動、監視、復旧、権限管理を増やすため不採用とする。
- 単一資源プールを無条件に共有する案は、新規Depositが既存Withdrawalの実行資源を消費できるため不採用とする。
- 単一資源プール内の論理予約と処理優先度を採用する。

## Consequences

- schedulerはRelease acknowledgementとBase RefundをDeposit mintより優先する。
- Settlement Reserveを満たせない場合、新規DepositをICP ledgerからpullする前に受付を停止する。
- 必要なSettlement Reserveは固定floorだけでなく、未完了Settlementに必要な保守的最大費用を含める。
- 受付済みDepositはICP tokenをpullする前に、対応するBase mintの保守的最大費用を予約する。
- Withdrawal受付を継続できない残高では、Base contractの新規Withdrawalをpauseし、既存Settlementだけを継続する。
- Verusで、Deposit受付がSettlement Reserveを侵食しないことと、Settlement taskがDeposit taskより優先されることを証明する。
- gas価格、EVM RPC費用、management canister call費用の上限評価は外部仮定として監査する。
- 論理予約は悪意あるcanister upgradeに対する物理隔離ではない。
