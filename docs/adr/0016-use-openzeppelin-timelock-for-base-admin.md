---
status: accepted
---

# Base Adminの実行境界にOpenZeppelin TimelockControllerを使う

Base Adminの危険方向操作はOpenZeppelin Contracts 5.6.1の`TimelockController`を経由する。
TimelockをBridgeより先にdeployし、初期minimum delayを24時間、Canister由来Governance Operatorをproposer、executor、canceller、追加adminを`address(0)`として構成する。
Timelock自身だけが`DEFAULT_ADMIN_ROLE`を持つ。構築後のTimelock role集合は凍結し、自己callを含むgrant、revoke、renounceを拒否する。role変更は、新しい承認済みrole集合のTimelockを配置してBridgeのTimelock rotationを行う。

## Considered Options

- Bridge内部に独自queueと時刻判定を持つ案は、Bridge ABIと監査対象を増やし、既存の検証済み実装を重複させるため採用しない。
- executorをpermissionlessにする案はschedule内容を書き換える権限を与えないが、本構成では実行主体をCanister由来Governance Operatorへ限定する方針を優先して採用しない。
- deployerへ暫定adminを付与する案はdelayを迂回できる期間を作るため採用しない。

## Consequences

- 外部EOAからBridgeのBase Admin関数を直接呼んでも失敗し、CanisterがTimelockへscheduleして24時間後にexecuteする。
- Bridgeは候補addressのbytecode、`getMinDelay() >= 24 hours`、候補自身の`DEFAULT_ADMIN_ROLE`保持をrotation時に検証する。
- proposer、executor、cancellerは同一のCanister由来Governance Operatorへ固定し、人間walletへroleを付与しない。role集合は構築後に凍結する。
