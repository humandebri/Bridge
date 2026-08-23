# KINIC–Base Bridge 実装計画

> 本文中の旧polling間隔と優先度schedulerは歴史的設計である。現在はADR 0019のstable settlement executor、ADR 0023のwallet送信型Mint Authorization、障害時だけのrate limit付き手動Retryを正本とする。

本計画は `docs/adr/` のADRと `docs/glossary.md` の用語定義に基づく。
用語は `docs/glossary.md` の定義に従い、本文では再定義しない。

SNS GovernanceをIC/Base双方の最終trust rootとする。Base操作はBridge Canisterが別derivation pathから導出するGovernance Operatorで署名し、外部relayerが送信する。人間のEVM管理鍵は置かない。
Bridge はKINICトークン専用にデプロイする（ADR 0010）。複数SNS tokenを扱う分岐は導入しない。
Mainnet Ledgerは`73mez-iiaaa-aaaaq-aaasq-cai`、Indexは`7vojr-tyaaa-aaaaq-aaatq-cai`に固定する。Archive canisterはLedgerから動的に発見する。

## 現在の進捗

Base contractのPhase 1EとPlan 001〜004は完了している。
Bridge canisterはstable schema v36、外部連携、Settlement Reserve、stable settlement executor、EIP-712 Mint Authorization、運用管理、Verus証明まで実装済みである。
Plan 005は10回・7日の本番パラメータ外部計測と単一emergency pause経路演習待ちである。Plan 006のSNS handover、Canister操作型Base管理、主要5 scenarioのGate B真正性検証、固定SNS activation proposal提出とpostcondition receipt経路は実装済みで、実mainnet evidenceの取得・承認・実行は未完了である。Plan 007のlocal staging構成とPocketIC/Anvil/frontend E2Eは実装済みで、追加wallet互換性と追加5 scenarioの外部実行は明示承認待ちだがproduction activationをblockしない。

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
Bridgeはconstructor内でbSNSを生成し（ADR 0014）、BaseのService FeeをcanisterがFinalized blockで読む正本とする（ADR 0013）。
EIP-3009の追加interfaceはPhase 1Aの正本とselector/topic testへ反映済みである（ADR 0015）。
Phase 1BでbSNS、EIP-3009、Deposit mint、Per-Deposit Limit、deploy時起点のfixed-window Mint Throughput Limitを実装済みである。
Phase 1CのWithdrawalは、`createWithdrawal`内でtransfer、burn、固定quoteを`Committed`へ原子的に記録する現在形へ置換済みである。Base側のacknowledgement、cancel、refundは存在しない。
Phase 1DでService Fee変更、独立pause、固定limit、role rotation、OpenZeppelinの24時間Timelock統合を実装済みである。
Phase 1Eで検証を閉じ、ABIを凍結済みである。

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
- 両制限とwindow長はdeploy時のimmutable値とし、raw unitで定義する。decimalsの表示変換を判定に使わない。
- Withdrawal IDからburnを取り消すBase refund/remint経路を持たない。Bridge Signerの通常Deposit mint権限は別のtrust assumptionとする。
- 各DepositにPer-Deposit Limitを適用し、同じfixed window内のmintを共有Mint Throughput Limitへ累積する。

### 1-4. Withdrawal 状態機械（ADR 0018）

- Base Withdrawalは`None → Committed`だけを持ち、`Committed`を不可逆な終端状態とする。
- `createWithdrawal`でtransfer、burn、Service Fee、`amountOut`、IC Accountを一つのtransactionへ固定する。
- ICP送金後のBase transactionやWithdrawal向けEVM operationを作らない。

### 1-5. Service Fee（ADR 0004）

- immutable な `MAX_SERVICE_FEE` を raw unit でデプロイ時に固定する。
- `0 <= service_fee <= MAX_SERVICE_FEE` を超える fee 変更を contract 側でも拒否する。
- Withdrawal の `maxServiceFee` と実行時Service Feeの比較により、処理中の fee 変更から利用者を保護する。
- WithdrawalのLedger FeeはBridgeが負担し、利用者の固定`amountOut`を減額しない。

### 1-6. 管理権限の分割（ADR 0005、0009）

- Withdrawal受付を継続できない残高を運用監視で検出したとき、Runtime Administratorが新規Withdrawalをpauseし、既存Settlementだけを継続する。Bridge contractやCanisterによる自動pauseは行わない。
- 即時操作（pause、上限内Service Fee変更）をRuntime Administratorのroleに割り当てる。
- 遅延操作（unpause、role rotation）はCanister由来Governance OperatorだけがTimelock経由で実行する。人間のEVM管理鍵とhardware walletは置かず、timelock遅延は24時間とする。
- limitを変更するfunctionとselectorは公開しない。
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

Phase 2で決定的状態機械と最初のstable schema、観測queryを実装した。
後続のPlan 002と003および現行ADRで外部連携、運用状態、settlement executor、fund-before-formal-deposit、wallet-funded EIP-712 Mint Authorizationを追加し、現行stable schemaはv36である。

### 2-1. state 設計（ADR 0008、0010）

- KINICトークン専用とし、state とデプロイ構成から token ID による分岐を排除する。
- 全 state を ic-stable-structures に直接保存し、`pre_upgrade` で全 serialize する設計を避ける。
- 未完了の Deposit、Withdrawal、EVM transaction、Reconciliation Hold を upgrade 後に再開できる表現にする。
- 本番初回deployまではstable schemaを直接置換し、migration、dual-read、fallbackを追加しない。現行version以外はfail closedとする。
- schema versionは`bridge_metadata`だけを正本とし、現行形式はschema v36・record wire v29とする。
- Deposit record、owner sequence、Base recipientは単一envelopeへ保存する。pending EVM、open hold、nonterminal Withdrawalの件数は対応indexのtable countを正本とする。
- Withdrawal primary rowとliability index、合計額、stop reason集計はtyped SQLite transactionで同時に更新し、change-log triggerへ依存しない。

### 2-2. Deposit フロー（ADR 0001、0004、0005）

1. 受付時はlocal pause、入力、`gross_amount > 100_000`を検査し、有料Base preflightより前に、正式Depositと分離したbounded funding attemptへ固定transfer identity、消費済みquota、active reservationを保存してcycle reserveを確認する。
2. admission成功後だけfresh Base preflightを行い、同じupdate callでICRC-2 pullを実行する。成功または`Duplicate`だけを正式Depositへ昇格する。BaseまたはLedgerの確定的失敗はattemptとactive reservationを削除するがquotaは戻さず、曖昧・callback消失は同じidentityでreconciliationする。
3. freshな観測でquoteとmint予約を原子的に確定する。観測不能・不一致・stale observationでは返金せず再観測する。
4. 認可発行前のBase pause、fee拒否、上限超過では`RefundAvailable`にし、任意の非anonymous callerの明示請求時にrecord固定の元accountへ`gross_amount - 100_000`を送る。認可発行後はstrict deadlineとcanonical未処理証拠を確認し、`gross_amount - charged_service_fee - 100_000`を送る。初回pull fee、確定service fee、refund Ledger feeは返さない。曖昧結果はRefund Reconciliation Holdへ移し、任意の非anonymous callerの再請求で照合する。
5. Mint Authorization署名の保存時だけService Feeをfee reserveへ一度計上する。Base mint成否でこのfeeを戻さず、fee payoutは確定済みreserveだけを使用する。

### 2-3. Withdrawal フロー（ADR 0004、0011、0018）

1. `createWithdrawal` receipt、単一event、`Committed`状態、Bridge signerとruntimeを同一2-of-3 quorum Finalized blockへ束縛して検証する。
2. 検証証拠、Withdrawal record、release job、transfer identity、監査eventを一つのSQLite transactionで保存してからICRC transferを開始する。
3. 固定`amountOut = amount - chargedServiceFee`を送り、Ledger FeeはBridgeが負担する。
4. transfer成功または`Duplicate`で`Paid`へ終端化し、結果不明はReconciliation Holdで完全履歴を照合する。

### 2-4. 会計の分離（ADR 0004、0005）

- fee reserve を Bridge Exposure の裏付け資産と分離して会計する。
- fee 送金は確定済み fee reserve だけを対象とし、裏付け資産に到達できない構造にする。
- Fee Recipient 変更時、未送金の確定済み fee reserve 全体を新 recipient へ帰属させる。recipient 別 bucket を持たない。

**完了条件**：mock 環境で Deposit と Withdrawal の全状態遷移が単体テストで検証される。

## Phase 3: 外部連携

Phase 3のICRC adapter、Base Finalized監視、EIP-712 Mint Authorization、Governance専用threshold ECDSA署名lane、外部Governance relayer、Reconciliation Hold履歴照合、公開Deposit APIは実装済みである。
PicJSでDeposit、Withdrawal、Holdのupgrade保持、stuck receiptを検証する。

### 3-1. EVM 連携（ADR 0005、0011）

- Deposit Mintはthreshold ECDSAでEIP-712 Authorizationへ署名し、Base walletがtransactionを送信する。Mint用nonce、raw transaction、gas reserveは持たない。
- 初期contract配置は外部EOAがTimelock、Bridgeの順に実行し、配置後にroleを残さない。Governance Operator laneではCanisterがnonceと署名済みgenerationを保持し、権限なしrelayerがbroadcast、Finalized待機、確定通知を行う。Canisterのrebroadcast、receipt timer、自動replacementは持たず、明示要求されたreplacementだけを同一nonceで最大3回再署名する。
- Withdrawalの受付観測は`eth_getLogs`で発見し、Finalized headの状態読みで確定する。読み取りは3 provider中2の合意を要求する。

### 3-2. Settlement Reserve と stable executor（ADR 0005、0019）

- cycles floorとsettlement cycle ceilingを署名、RPC、Ledger処理のために維持し、ETH floorはGovernance Operator専用とする。
- 未処理Authorizationの論理Mint capacityをterminal状態まで予約するが、Deposit admissionへMint gasやETH reserveを含めない。
- stable executorのjobは型付きkindごとのclaim policyとrecord単位leaseを持ち、Deposit、Withdrawal、fee payoutを混同しない。
- lease generationは単調増加とし、stale callback、同一recordの重複claim、進行中scheduled jobのgeneric manual claimを拒否する。automatic、public manual、Governance recovery laneは独立した上限を持つ。
- cycles制約または論理Mint capacityを満たせないとき、新規Depositの受付を停止する。
- Governance gas価格、EVM RPC費用、management canister call費用の上限評価を外部仮定として文書化し、監査対象にする。

Settlement Reserve、stable executor、新規Deposit pause、Fee Recipient、fee payout、stable監査ログはPlan 003およびADR 0019の構成で実装済みである。
本番の数値と鍵保管方式はPlan 005と006で確定する。

### 3-3. Reconciliation Hold（ADR 0006）

- deduplication 期間内は、同一の `created_at_time`、memo、amount、fee、from、to、spender でだけ再試行する。
- 期間経過後は ICRC-3 と index 履歴で照合する。archive を含む検索範囲の完全性と同期済み watermark を確認し、memo 単独で判定しない。
- 履歴サービスの遅延、欠落、archive 障害がある間は「存在しない」と判定しない。
- 成否を確定できない要求を Reconciliation Hold へ無期限に留め、時間経過による再送、Deposit 返金、Base Refund を禁止する。
- Governance による解除は証拠に基づく成否確定に限定し、証拠なしの再送と返金を強制できない API にする。

**完了条件**：PocketIC とローカル EVM ノードによる統合テストで、正常系、失敗系、Reconciliation Hold 遷移が検証される。

## Phase 4: 管理権限

Plan 003で管理権限と監査ログを実装済みである。

- 単一pause principalはIC/Base双方のpause、記録済みpending Timelock cancel、許可済みSettlementの進行だけを実行できる。
- SNS Governanceだけが再開、pause principal rotation、Fee Recipient、fee payout、Service Fee、Timelock schedule/executeを実行できる。
- Base操作はMint Signer laneとGovernance Operator laneを分離し、任意target/calldata/raw transaction/nonce APIを公開しない。
- 人間のEVM address、controller identity、初回deployerへ永続roleを与えない。
- SNS-token feeからBase gas用ETHへの自動変換は行わず、運用者がrunbookに従って補充する。

## Phase 5: 形式検証（Verus）

Plan 004でproduction共有kernelの証明とnegative fixtureを実装済みである。
証明はWasmごとに再実行し、過去版の証明を新しいupgradeへ流用しない（ADR 0008）。

- 各DepositのquoteがService Fee上限、正のnet額、Per-Deposit Limit、Mint Throughput Limitを満たし、quote確定時だけmint予約へ移ること。Withdrawal専用のBase refund/remint経路がなく、処理済みDeposit IDをreplayできないこと（ADR 0018、0021）。
- canonical観測とLedger成功を前提に、1件のWithdrawalがBase `Committed`からCanister `Paid`へ進み、`Paid`後に再送・減額・送金先変更されないこと（ADR 0018）。外部サービスのlivenessは主張しない。
- Service Feeの上限制約、二重計上防止、成功前のfee確定禁止、未完了payoutがない場合のrecipient変更によるreserve保存、fee reserveを超える送金の禁止（ADR 0004）。
- Deposit受付がcandidateを含むSettlement Reserveを満たし、candidateからreservedへの移行で必要資源量を減らさないこと（ADR 0005）。
- stable settlement executorのleaseがrecordとlaneへ束縛され、generationが単調増加し、stale callback、同一recordの重複claim、scheduled/leased jobの手動迂回を拒否すること（ADR 0019、0023）。
- release対象claimは`Claims.lean`、有限幅semanticsは`Implementation.lean`、implementation対応は`Refinement.lean`、統合traceは`Protocol.lean`へ分離し、claim台帳、vector section、production consumer、外部仮定をCIで完全一致させる。release driverは自己申告attestationを受理せず、不可逆操作直前にclean sourceからproof gateと二重artifact buildを再実行する。
- Reconciliation Hold から新規 transfer または補償状態へ直接遷移しないこと（ADR 0006）。

証明範囲は資産の 1:1 裏付けと上記の性質に限定し、cross-chain governance を含めない（ADR 0002）。

## Phase 6: SNS 移管と本番準備

- 開発者 identity が controller である間は Bridge を未稼働または全面 pause とし、本番 SNS トークンを pull しない（ADR 0008）。
- upgrade 前後で未完了のDeposit Authorization、Withdrawal、Governance EVM transaction、Reconciliation Holdが再開できることを、実データ相当のstateで検証する。
- handover を実行し、controller 一覧が SNS Root だけであることを確認する。開発者 identity、fallback identity、NNS Root を残さない。
- handover 後の upgrade proposal に添付する成果物（Wasm hash、source revision、Verus 結果、テスト結果、stable schema 互換性）の生成を CI で自動化する。
- EIP-3009はbSNSの任意連携機能とし、x402 resource serverやfacilitatorとの互換性をBridgeの配置・activation条件に含めない（ADR 0015）。
- UI 側の要件として、Deposit 前に bSNS では投票と投票報酬を得られないことを明示する（ADR 0002）。UI 実装が別リポジトリの場合は要件として引き渡す。

**完了条件**：handover checklist がすべて満たされ、SNS proposal による upgrade が一度実際に成功する。

## 未完了事項

Plan 005の完了には、Sepoliaでのgovernance gasとsettlement cycles各10回、Base mainnetの7日fee分布、承認済み日次settlement上限、単一pause principalの実request/audit証跡、固定limitの承認、pause/cancel経路演習が必要である。cycles floorは基礎日次消費と10回計測最大値を用いる30日負荷モデルへ2倍の安全係数を掛けて導出する。5/15/60は本番ゲートではなく公開後の監視目標とする。
これらの証跡が揃うまでmainnet candidateを`validated`にしない。
Plan 006のrepository実装は完了している。完了判定には、SNS Rootへの実controller handover、実upgrade proposal、認証済みGate A/Gate B、schedule/execute activation receiptをmainnet evidenceとして取得する必要がある。

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
