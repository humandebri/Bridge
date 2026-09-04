---
status: accepted
---

# Settlement用cyclesとMint capacityを論理予約する

Bridgeは単一canisterのcycles残高を共有する。新規Depositが既存operationのSettlement用cyclesを侵食しないよう、非終端liabilityの保守的最大費用を論理予約する。未処理Mint Authorizationのcapacityも終端状態まで別に予約する。Base control-planeはGovernance Operator、Runtime Administrator、Independent Cancellerごとにsigner、nonce lane、ETH残高を分離し、送信候補transactionごとに必要liabilityを検査する。Deposit MintのBase gasは利用者walletが負担するため、Deposit admissionのETH reserveには含めない。

## Considered Options

- DepositとSettlementでcanisterを物理分離する案は、資金移動、監視、復旧、権限管理を増やすため不採用とする。
- cyclesとMint capacityを無条件に共有する案は、新規Depositが既存operationの実行資源を消費できるため不採用とする。
- 共有cycles残高内の論理予約、Mint capacity予約、record指定の明示操作、Base control-plane roleの物理分離を採用する。

## Consequences

- Settlementは利用者または管理者が指定したrecordだけを処理する。
- Settlement Reserveを満たせない場合、新規DepositをICP ledgerからpullする前に受付を停止する。
- 必要なSettlement cycles reserveは運用floorに加え、すべての非終端liabilityの保守的最大費用を含める。
- 受付済みDepositは対応するMint capacityを終端状態まで予約するが、Base mint gas用ETHは予約しない。
- Base control-plane transactionは、選択されたsender roleのFinalizedとSafeの保守的なETH残高がcandidate liabilityを満たす場合だけ署名・送信する。別の固定ETH floorは設けない。
- Withdrawal受付を継続できない残高では、Base contractの新規Withdrawalをpauseし、既存Settlementだけを継続する。
- Verusで、Deposit受付がSettlement Reserveを侵食しないことを証明する。
- gas価格、EVM RPC費用、management canister call費用の上限評価は外部仮定として監査する。
- 論理予約は悪意あるcanister upgradeに対する物理隔離ではない。
