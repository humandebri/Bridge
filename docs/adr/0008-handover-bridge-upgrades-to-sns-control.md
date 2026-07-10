---
status: accepted
---

# Bridge canisterをupgrade可能にしてSNS管理へ移管する

Bridge canisterはupgrade可能にする。開発・初期検証中は開発者identityをcontrollerとし、本番資産の受付前にSNS管理へ移管する。移管後はSNS Rootを唯一のcontrollerとし、SNS Governanceの採択proposalだけがupgradeを承認する。

## Considered Options

- controllerを除去してBridge canisterをimmutableにする案は、stable state障害、IC API変更、依存更新へ対応できないため不採用とする。
- 開発者identityを移管後もco-controllerとして残す案は、SNS proposalを経ずにupgradeできるため不採用とする。
- SNS Governanceをcontrollerへ直接設定する案は、SNSの標準構成ではRootがapp canisterのcontrollerとしてupgradeを実行するため不採用とする。
- SNS Rootを唯一のcontrollerとし、upgrade権限をSNS Governance proposalへ委ねる案を採用する。

## Consequences

- 開発者identityがcontrollerである間はBridgeを未稼働または全面pauseとし、本番SNS tokenをpullしない。
- handover完了条件はcontroller一覧がSNS Rootだけであることとし、開発者identity、fallback identity、NNS Rootを残さない。
- handover後のupgradeはSNS proposalにWasm hash、source revision、Verus結果、テスト結果、stable schema互換性を添付する。
- Rust stateはstable structuresへ直接保存し、全stateを`pre_upgrade`でserializeする設計を避ける。
- upgrade前後で未完了Deposit、Withdrawal、EVM transaction、Reconciliation Holdを再開できることを検証する。
- Runtime Administratorはcanister controllerにしない。pause、Service Fee、Fee Recipientの変更権限とupgrade権限を分離する。
- SNS GovernanceはupgradeによりBridgeロジックを変更できるため、ICP側コードの最終的な信頼主体である。この権限はBase contractのimmutable制約を変更できない。
- Verusの証明はWasmごとに再実行し、過去版の証明を新upgradeへ流用しない。
