---
status: accepted
---

# Bridge canisterをupgrade可能にしてSNS管理へ移管する

Bridge canisterはupgrade可能にする。初回activationと本番計測中はproduction identityを単独controllerとして保持できる。初回executeのConfirmed完了は内部bootstrap activation authorityだけを永久に消費し、外部controller設定の変更時期を決めない。運用者が別途承認して移管する場合はSNS Rootを唯一のcontrollerとし、以後はSNS Governanceの採択proposalだけがupgradeを承認する。

## Considered Options

- controllerを除去してBridge canisterをimmutableにする案は、stable state障害、IC API変更、依存更新へ対応できないため不採用とする。
- 開発者identityを移管後もco-controllerとして残す案は、SNS proposalを経ずにupgradeできるため不採用とする。
- SNS Governanceをcontrollerへ直接設定する案は、SNSの標準構成ではRootがapp canisterのcontrollerとしてupgradeを実行するため不採用とする。
- SNS Rootを唯一のcontrollerとし、upgrade権限をSNS Governance proposalへ委ねる案を採用する。

## Consequences

- production identityがcontrollerであること自体は、初回activation後の本番資産受付を禁止しない。受付可否はGate B、Confirmed execute、pause状態と運用limitで決める。
- unpause後の7日・各10件の本番計測と`fee-cycles-measurements.json`はhandoverの認可入力にしない。移管は初期運用値、seal／schedule／execute receipt、live RuntimeBinding、current profile Wasmへ束縛し、別の明示承認を必要とする。
- handover送信直前はproduction identityだけをcontrollerとし、ActivatedかつBase Deposit／WithdrawalとIC Depositをすべてunpausedにする。完了条件はcontroller一覧がSNS Rootだけであることとし、開発者identity、fallback identity、NNS Rootを残さない。
- 初回install hashとlive moduleを同一視せず、post-Gate-A policy transitionと通常upgrade receiptからcurrent profile Wasmまでのchainを検証する。
- controller変更前後のmodule、RuntimeBinding、storage integrity、activation／pause状態、record／audit countをraw evidenceへ保存してcontinuityを検証する。運用中stateの空化は要求しない。
- handover後のupgradeはSNS proposalにWasm hash、source revision、Verus結果、テスト結果、stable schema互換性を添付する。
- Rust stateはstable structuresへ直接保存し、全stateを`pre_upgrade`でserializeする設計を避ける。
- upgrade前後で未完了Deposit、Withdrawal、EVM transaction、Reconciliation Holdを再開できることを検証する。
- Runtime Administratorはcanister controllerにしない。pause、Service Fee、Fee Recipientの変更権限とupgrade権限を分離する。
- SNS GovernanceはupgradeによりBridgeロジックを変更できるため、ICP側コードの最終的な信頼主体である。この権限はBase contractのimmutable制約を変更できない。
- Verusの証明はWasmごとに再実行し、過去版の証明を新upgradeへ流用しない。
