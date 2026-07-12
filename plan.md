# SNS–Base Bridge 実装計画

本計画は `docs/adr/` の ADR 0001〜0016 と `CONTEXT.md` の用語定義に基づく。
用語は CONTEXT.md の定義に従い、本文では再定義しない。

ADR 0007（SNS Governance を Base admin の権限主体にする）は ADR 0009 により supersede された。
Base contract の admin 権限は Governance Executor ではなく、安全方向（Runtime Administrator の高速パス）と危険方向（Base Admin の timelock 経由）の分割で実装する。
Bridge は単一 SNS トークン専用にデプロイする（ADR 0010）。

## 全体構成

実装対象は次の 3 つのコンポーネントである。

- **Base contract 群**：bSNS ERC-20 と Bridge contract。デプロイ後にアップグレードしない（ADR 0001）。
- **Bridge canister**：ICP 側の Rust canister。escrow、Deposit と Withdrawal の状態機械、EVM への署名送信を担う。アップグレード可能とし、本番前に SNS 管理へ移管する（ADR 0008）。
- **形式検証**：canister 側は Verus、contract 側は Solidity SMTChecker。各 ADR が指定する証明義務を対象とする。

依存関係は次のとおりである。
Base contract のインターフェース（イベント、Withdrawal 状態、fee 制約）が canister 側の状態機械の前提になるため、contract を先に確定させる。
形式検証は各コンポーネントの実装と並走させ、後付けにしない。

## Phase 0: 基盤整備

- リポジトリ構成を決める（`contracts/`、`canister/`、`docs/` の分離）。
- Solidity 側のツールチェーン（Foundry と SMTChecker）を整備する。
- Rust 側のツールチェーン（ic-cdk、ic-stable-structures、Verus）を整備する。
- ローカル実行環境を用意する（PocketIC と anvil、または同等の EVM ローカルノード）。
- CI で build、test、SMTChecker、Verus を回す骨格を作る。

**完了条件**：空実装の contract と canister がローカルで deploy でき、CI が通る。

## Phase 1: Base contract

bSNS ERC-20 と Bridge contract を実装する。
アップグレード不能である以上、この Phase の設計ミスは再デプロイでしか直せない。
ADR が contract 側へ課す制約をすべてこの Phase で実装する。

Phase 1Aで確定したconstructor、型、関数、event、error、権限表は`docs/base-interface.md`を正本とする。
Bridgeはconstructor内でbSNSを生成し（ADR 0014）、BaseのService Feeをcanisterがfinalized blockで読む正本とする（ADR 0013）。
EIP-3009の追加interfaceはPhase 1Aの正本とselector/topic testへ反映済みである（ADR 0015）。
Phase 1BでbSNS、EIP-3009、Deposit mint、Per-Deposit Limit、deploy時起点のfixed-window Mint Throughput Limitを実装済みである。
Phase 1CでWithdrawal burn、Release acknowledgement、ledger block index一意性、Base Refund、settlement fee記録を実装済みである。
Phase 1DでService Fee変更、独立pause、limit変更、role rotation、OpenZeppelinの72時間Timelock統合を実装済みである。
Phase 1Eで検証を閉じてABIを凍結する。

### 1-1. bSNS ERC-20

- Bridgeable SNS Token を 1:1 で裏付ける ERC-20 とする（ADR 0002）。
- 投票権、neuron 権限、Governance 用 identity mapping をいっさい持たせない（ADR 0002）。
- mint と burn の権限を Bridge contract に限定する。

### 1-2. EIP-3009署名送金（ADR 0015）

- `transferWithAuthorization`と`receiveWithAuthorization`を実装し、x402 `exact`決済がbSNSを直接settleできるようにする。
- `authorizationState`と`cancelAuthorization`を実装し、使用済みと取消済みのnonceをauthorizerごとの単一namespaceで再利用できないようにする。
- EIP-712 domainをtoken name、固定version `"1"`、実行chain ID、bSNS contract addressへ束縛する。
- `receiveWithAuthorization`ではcallerと受取人の一致を要求する。
- authorization送金は既存balanceの移転だけに限定し、Bridge以外へmintとburnの権限を与えない。
- Foundryで正常送金、replay、期限の前後、署名者とdomainの不一致、取消し、`receiveWithAuthorization`の受取人検査をテストする。
- 標準ERC-20のallowanceを維持し、Permit2を代替のx402決済経路として妨げない。

### 1-3. Deposit mint の流量制御（ADR 0001、0012）

- Per-Deposit Limit を各 Deposit に適用する。
- Mint Throughput Limit を fixed window（初期値 1 時間）の新規 deposit mint 総量に適用する。window 境界バーストの 2 倍係数は上限値の導出（`docs/parameters.md`）で織り込む。
- 両制限とも raw unit で定義し、decimals の表示変換を判定に使わない。
- refund mint は新規 deposit mint に計上せず、両制限の対象外とする。
- mint batch では、各 Deposit に Per-Deposit Limit を適用したうえで、batch 全体を Mint Throughput Limit に算入する。

### 1-4. Withdrawal 状態機械（ADR 0003）

- Withdrawal の状態を `Pending`、`Released`、`Refunded` とし、`Pending → Released` と `Pending → Refunded` を排他にする。
- Release acknowledgement を withdrawal ID で冪等にし、同一内容の再実行を成功扱いにする。
- Base Refund は `Pending` の Withdrawal だけに許可する。

### 1-5. Service Fee（ADR 0004）

- immutable な `MAX_SERVICE_FEE` を raw unit でデプロイ時に固定する。
- `0 <= service_fee <= MAX_SERVICE_FEE` を超える fee 変更を contract 側でも拒否する。
- Withdrawal の `minAmountOut` により、処理中の fee 変更から利用者を保護する。
- cancel と Base Refund では Service Fee を徴収しない。

### 1-6. 管理権限の分割（ADR 0005、0009）

- Withdrawal 受付を継続できない残高のとき、新規 Withdrawal を pause し、既存 Settlement だけを継続できる構造にする。
- 安全方向の操作（pause、limit 引き下げ）を Runtime Administrator の role に割り当てる。
- 危険方向の操作（unpause、limit 引き上げ、role rotation）を timelock 経由の Base Admin（Safe multisig）に割り当てる。timelock 遅延は初期値 72 時間とし、遅延短縮と signer 変更も timelock を経由する。
- Base Admin に mint、refund、escrow 資産への権限を与えない。

### 1-7. SMTChecker による証明（ADR 0004）

- Service Fee の上限制約。
- fee の二重計上防止と、成功前の fee 確定禁止。
- recipient 変更時の fee reserve 保存。
- fee reserve を超える送金の禁止。

**完了条件**：Foundry テストと SMTChecker が通り、Phase 1EのABI snapshotとfixtureでインターフェース（イベント、関数シグネチャ）を凍結できる。

### 1-8. Contract検証とABI凍結

- concrete `Bridge`と`BSNS`のcanonical ABI snapshotを追跡し、interface subsetとconstructor、struct、enumの形状をCIで検査する。
- Foundryのfuzzを1000 runs、stateful invariantを256 runs・depth 100・`fail_on_revert`で実行する。
- Deposit mint、Withdrawal exposure、terminal state、roleとfee safetyをproduction共有library、unit test、stateful invariantで検証する。
- EIP-3009 authorizationのnonce namespaceとrollbackをunit・fuzz testで検証する。
- coverage summaryを情報表示として保存し、数値閾値ではなく未検証経路を検査対象として管理する。
- 証明義務と外部仮定を`verification/obligations.md`へ記録する。

**完了条件**：ABI snapshot、selector/topic fixture、Foundry fuzz/invariant、SMT pass/negative、coverage summary、local smoke、CIが同一判定で通り、Phase 1E以後のABI変更は別計画と再レビューを要する。

## Phase 2: Bridge canister の状態機械

Deposit と Withdrawal の状態機械を、外部呼び出しを mock した純粋なロジックとして先に実装する。
外部呼び出し（ICRC ledger、EVM RPC、threshold ECDSA）を分離しておくのは、Verus の証明対象を決定的なロジックに限定するためである。

### 2-1. state 設計（ADR 0008、0010）

- 単一 SNS トークン専用とし、state とデプロイ構成から token ID による分岐を排除する。
- 全 state を ic-stable-structures に直接保存し、`pre_upgrade` で全 serialize する設計を避ける。
- 未完了の Deposit、Withdrawal、EVM transaction、Reconciliation Hold を upgrade 後に再開できる表現にする。
- stable schema の互換性を検証するテストを最初から用意する。

### 2-2. Deposit フロー（ADR 0001、0004、0005）

1. 受付時に、対応する Base mint の保守的最大費用を予約できるか検査する。予約できなければ ICP ledger から pull する前に受付を拒否する。
2. ICRC-2 で SNS トークンを escrow へ pull する。利用者指定の `max_service_fee` を検査する。
3. Base mint 量は、ロック量から Service Fee を引いた量とする。
4. mint 成功時にのみ Service Fee を fee reserve へ確定する。

### 2-3. Withdrawal フロー（ADR 0003、0004、0011）

1. Base の Withdrawal を `finalized` の contract 状態読みで確定し、受理する。イベントログは発見にだけ使う。
2. ICP Release では、burn 量から ledger fee と Service Fee を引いた量を ICRC transfer する。
3. transfer の成功、`Duplicate`、または完全な履歴照合を確定してから、Base へ Release acknowledgement を送る。
4. ICP Release 開始後、Base で `Released` が finalize するまで自動 refund へ遷移させない。
5. Release 成功時にのみ Service Fee を fee reserve へ確定する。

### 2-4. 会計の分離（ADR 0004、0005）

- fee reserve を Bridge Exposure の裏付け資産と分離して会計する。
- fee 送金は確定済み fee reserve だけを対象とし、裏付け資産に到達できない構造にする。
- Fee Recipient 変更時、未送金の確定済み fee reserve 全体を新 recipient へ帰属させる。recipient 別 bucket を持たない。

**完了条件**：mock 環境で Deposit と Withdrawal の全状態遷移が単体テストで検証される。

## Phase 3: 外部連携

### 3-1. EVM 連携（ADR 0005、0011）

- threshold ECDSA による署名と、EVM RPC canister 経由の transaction 送信を実装する。
- nonce queue は単一とする。
- Withdrawal settlement 用の Base gas を新規 Deposit 処理と別に確保する（ADR 0003）。
- Withdrawal の観測は `eth_getLogs` で発見し、`finalized` タグの状態読みで確定する。読み取りは 3 provider 中 2 の合意を要求し、polling は timer 1〜5 分間隔とする。

### 3-2. Settlement Reserve と scheduler（ADR 0005）

- ETH と cycles の一部を Settlement Reserve として会計上予約する。
- 必要量は固定 floor に加え、未完了 Settlement の保守的最大費用を含めて算出する。
- scheduler は Release acknowledgement と Base Refund を Deposit mint より優先する。
- Settlement Reserve を満たせないとき、新規 Deposit の受付を停止する。
- gas 価格、EVM RPC 費用、management canister call 費用の上限評価を外部仮定として文書化し、監査対象にする。

### 3-3. Reconciliation Hold（ADR 0006）

- deduplication 期間内は、同一の `created_at_time`、memo、amount、fee、from、to、spender でだけ再試行する。
- 期間経過後は ICRC-3 と index 履歴で照合する。archive を含む検索範囲の完全性と同期済み watermark を確認し、memo 単独で判定しない。
- 履歴サービスの遅延、欠落、archive 障害がある間は「存在しない」と判定しない。
- 成否を確定できない要求を Reconciliation Hold へ無期限に留め、時間経過による再送、Deposit 返金、Base Refund を禁止する。
- Governance による解除は証拠に基づく成否確定に限定し、証拠なしの再送と返金を強制できない API にする。

**完了条件**：PocketIC とローカル EVM ノードによる統合テストで、正常系、失敗系、Reconciliation Hold 遷移が検証される。

## Phase 4: 管理権限

- Runtime Administrator を実装する。操作範囲は pause、limit 引き下げ、上限内の Service Fee 変更、Fee Recipient 変更に限定し、mint、refund、任意送金の権限を含めない（ADR 0004、0008、0009）。
- pause は複数 principal（運用者の鍵と監視 canister）から発動可能にする。安全方向の操作は発動しやすいほどよい。
- fee と recipient の変更はハードウェア鍵 1 本に限定し、admin principal 自体の rotation は SNS Governance のみが実行できる形にする（暫定運用方針。詳細は Phase 6 前に確定）。
- Runtime Administrator を canister controller にしない（ADR 0008）。
- Fee Recipient と Service Fee の変更をイベントと監査ログへ記録する（ADR 0004）。
- SNS-token fee から Base gas 用 ETH への自動変換は実装せず、運用者による ETH 補充手順を運用文書として別途書く（ADR 0004）。

## Phase 5: 形式検証（Verus）

各 ADR が指定する証明義務を Verus で証明する。
証明は Wasm ごとに再実行し、過去版の証明を新 upgrade へ流用しない（ADR 0008）。

- 各 Deposit が Per-Deposit Limit を超えないこと。mint 流量の消費量が保存されること。refund が新規 deposit mint に計上されないこと（ADR 0001）。
- 1 件の Withdrawal が `Released` と `Refunded` の両方へ到達しないこと（ADR 0003）。
- Service Fee の上限制約、二重計上防止、成功前の fee 確定禁止、recipient 変更時の reserve 保存、fee reserve を超える送金の禁止（ADR 0004）。
- Deposit 受付が Settlement Reserve を侵食しないこと。Settlement task が Deposit task より優先されること（ADR 0005）。
- Reconciliation Hold から新規 transfer または補償状態へ直接遷移しないこと（ADR 0006）。

証明範囲は資産の 1:1 裏付けと上記の性質に限定し、cross-chain governance を含めない（ADR 0002）。

## Phase 6: SNS 移管と本番準備

- 開発者 identity が controller である間は Bridge を未稼働または全面 pause とし、本番 SNS トークンを pull しない（ADR 0008）。
- upgrade 前後で未完了の Deposit、Withdrawal、EVM transaction、Reconciliation Hold が再開できることを、実データ相当の state で検証する。
- handover を実行し、controller 一覧が SNS Root だけであることを確認する。開発者 identity、fallback identity、NNS Root を残さない。
- handover 後の upgrade proposal に添付する成果物（Wasm hash、source revision、Verus 結果、テスト結果、stable schema 互換性）の生成を CI で自動化する。
- 採用時点のx402 SDKとBase上のfacilitatorを使い、EIP-3009によるbSNSのverifyとsettleをtestnetで確認する。
  x402 resource serverとfacilitatorの運用は本Bridgeの責務に含めない（ADR 0015）。
- UI 側の要件として、Deposit 前に bSNS では投票と投票報酬を得られないことを明示する（ADR 0002）。UI 実装が別リポジトリの場合は要件として引き渡す。

**完了条件**：handover checklist がすべて満たされ、SNS proposal による upgrade が一度実際に成功する。

## 未決事項

当初の未決事項 5 件のうち 4 件は解決済みである。

1. **Base contract の admin 権限主体**：解決済み。ADR 0009（安全方向と危険方向の権限分割）が ADR 0007 を supersede した。
2. **パラメータの具体値**：導出式は `docs/parameters.md` に確定した。数値は対象 SNS トークンの確定後に同文書の TBD を埋める。
3. **対象 SNS トークンの単複**：解決済み。単一トークン専用にデプロイする（ADR 0010）。どの SNS トークンを対象とするかは残タスク。
4. **burn イベント観測の方式**：解決済み。`finalized` の状態読みで確定する（ADR 0011）。
5. **Runtime Administrator の鍵管理**：暫定運用で進める。pause は複数 principal から許可、fee と recipient の変更はハードウェア鍵 1 本、admin principal の rotation は SNS Governance のみ、という方針を Phase 4 に反映済み。保管方式の詳細は機能完成後、Phase 6 の前に確定する。

## Phase 間の依存とマイルストーン

| Phase | 内容 | 前提 |
|---|---|---|
| 0 | 基盤整備 | なし |
| 1 | Base contract | Phase 0 |
| 2 | canister 状態機械 | Phase 1 のインターフェース凍結 |
| 3 | 外部連携 | Phase 2 |
| 4 | 管理権限 | Phase 2（Phase 3 と並行可） |
| 5 | Verus 証明 | Phase 2 以降と並走 |
| 6 | SNS 移管 | Phase 1〜5 すべて、対象 SNS トークンの確定、`docs/parameters.md` の TBD 解消、鍵管理詳細の確定 |
