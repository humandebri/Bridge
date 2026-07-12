---
status: accepted
---

# Base Adminの実行境界にOpenZeppelin TimelockControllerを使う

Base Adminの危険方向操作はOpenZeppelin Contracts 5.6.1の`TimelockController`を経由する。
TimelockをBridgeより先にdeployし、初期minimum delayを72時間、Safe multisig addressだけをproposer、canceller、executor、追加adminを`address(0)`として構成する。
Timelock自身だけが`DEFAULT_ADMIN_ROLE`を持ち、delayとroleの変更もschedule済みの自己callで実行する。

## Considered Options

- Bridge内部に独自queueと時刻判定を持つ案は、Bridge ABIと監査対象を増やし、既存の検証済み実装を重複させるため採用しない。
- executorをpermissionlessにする案はschedule内容を書き換える権限を与えないが、本構成では運用主体をSafeへ限定する方針を優先して採用しない。
- deployerへ暫定adminを付与する案はdelayを迂回できる期間を作るため採用しない。

## Consequences

- SafeからBridgeのBase Admin関数を直接呼んでも失敗し、SafeはTimelockへscheduleして72時間後にexecuteする。
- Safeが利用不能な間はready済み操作もexecuteできない。
  このliveness tradeoffを受け入れる。
- Bridgeは`baseAdminTimelock` addressのbytecode、delay、role構成を内部検証しない。
  正しいTimelockを指定したことはdeploy smokeと本番preflightの外部仮定とする。
- Safe本体、owner threshold、Safe signer変更をtimelockへ拘束する具体構成はPhase 6の本番開始条件とし、Phase 1DではSafe相当addressで統合試験する。
