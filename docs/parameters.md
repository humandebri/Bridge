# パラメータ導出

Bridge の安全パラメータの導出式と初期値を記録する。
すべて raw unit で定義し、token decimals の表示変換を判断に使わない（ADR 0001）。
Mint limitとwindow長はdeploy時に固定し、どの権限にも変更を許可しない。

## Mint Throughput Limit

fixed window（ADR 0012）で実装するため、window 境界をまたぐ短時間に最大で上限の 2 倍がmintされうる。

```
window あたりの上限 = 許容最大被害額 ÷ 2
許容最大被害額 = 監視が pause を発動するまでの想定時間内に失ってよい額
```

- window 長の初期値: 1 時間
- production初期上限値: `15000000000000` raw（150,000 KINIC）。承認済みdeployment profileへ同じ値を明示し、deploy後は変更しない。

監視はwindow長とは別に、異常を5分以内に検知し、15分以内に担当者が確認し、60分以内にBaseとICの双方をpauseする。
監視体制がこれを満たせないなら、window を延ばすのではなく上限値を下げる。

## Per-Deposit Limit

```
Per-Deposit Limit = 承認済みdeployment profileの固定値
```

- production初期値: `15000000000000` raw（150,000 KINIC、2026年7月17日時点の総供給量の約2.5%）。
- Mint Throughput Limit初期値も`15000000000000` rawとし、1件で1時間window全量を消費できる。
- 固定window境界をまたぐ短時間最大量は2 window分の300,000 KINIC（総供給量の約5%）となる。

Mint Throughput Limit が総量を抑えるため、この値は単発の入力ミスと単一要求の異常検知を目的とする。

## MAX_SERVICE_FEE

immutable であり、デプロイ後にいかなる権限でも変更できない（ADR 0004）。
将来の価格変動に耐えるよう保守的に高く置き、運用値の `service_fee` はその下から始める。

```
MAX_SERVICE_FEE = 10 KINIC
service_fee初期値 = 0.5 KINIC
```

- KINIC ledger fee: `100000` raw
- Base Sepolia staging TICRC1 ledger fee: `10000` raw（`test-deployment` buildのみ）
- `MAX_SERVICE_FEE`: `1000000000` raw（10 KINIC、ledger feeの10000倍）
- `service_fee`運用初期値: `50000000` raw（0.5 KINIC、ledger feeの500倍）

## Settlement Reserve

固定 floor に未完了 Settlement の保守的最大費用を加算する（ADR 0005）。

```
必要 Settlement Reserve =
    固定 floor
  + Σ (未完了 Settlement ごとの gas limit × max fee per gas 上限)
  + cycles の N 日分の運用費
```

- ETH固定floor: 未確定。Sepolia governance gas 10回計測とBase mainnet 7日fee分布の証跡が揃った後、承認済みreserve window内のGovernance transaction数へ2倍の余裕を掛けて設定する。
- max fee per gas上限: 未確定。Base mainnet直近7日のbase fee p99×20、priority fee p95×4、L1 fee p99×10で各ceilingを算出する。
- cycles floor: 未確定。pause状態の基礎日次消費、10回のsettlement cycles計測、承認済み日次最大件数から次式で設定する。

production installとGate Aでは、上記3値が未確定のためschema 2 template固定のBootstrap運用値を使う。この値は運用上限ではなく、`Bootstrap` lifecycleとshared kernel gateの組でasset update、scheduler、Base governance transactionをfail closedにするための非運用値である。Baseをpause配置した後に計測を完了し、Gate B profileで3値だけを最終値へ置換して一度だけ封印する。
- `cycles floor = (baseline cycles/day + max(settlement cycles) × expected daily settlements) × 30 × 2`
- `settlement cycle ceiling = ceil(max(settlement cycles) × 1.5)`
- N: 30日

未確定値をzeroや任意の仮値でmainnet plan/profileへ入れてはならない。install時だけはprotocol定義済みの固定Bootstrap sentinelを使用する。production Canister install planは`schema_version: 2`、install receiptは`schema_version: 3`、release profileは`schema_version: 5`、Gate A/B release manifestは`schema_version: 3`、Gate A receiptは`schema_version: 2`、Activation Receiptは`schema_version: 4`だけを受理し、旧versionや未知versionをmigrationせずfail closedにする。`bridge-profile validate-bundle --offline`、`validate-bundle --offline --gate-b`、`verify-live`は、実artifact、署名、zero reserve、証跡欠落をfail closedで拒否する。Gate Bのoffline検証結果は`authorizing=false`であり、proofと再build後の`verify-live`だけがactivation proposalを認可する。

## timelock 遅延（Base Admin）

- 初期値: 24 時間（ADR 0016）
- 短縮は timelock 自身を経由する。

## 外部仮定の監査リスト

以下は Bridge 内部で保証できず、値の妥当性を運用監査で維持する（ADR 0005、0011）。

- gas 価格の上限評価
- Base governance transactionはCanisterが署名し、外部relayerが送信・Finalized待機・確定通知を行う。自動再送・自動replacementは行わない。運用者が明示要求した場合だけ同一nonce・payloadで最大3回、直前generationから12.5%以上fee bumpし、設定済みceilingを超えないtransactionをCanisterが再署名する。各署名前に`gas_limit × max_fee_per_gas + l1_fee_per_transaction_ceiling_wei + value`をchecked計算し、Safe/Finalized残高の小さい方が不足する場合は状態を変更せず拒否する。
- EVM RPC 費用と management canister call 費用の上限評価
- Settlementの一時障害retryはGovernance timerと共有せず、`settlement_retry_interval_seconds`（初期値60秒）を基準に指数backoffし、最大15分とする。
- 公式EVM RPC Canisterと設定されたquorumがcanonical Finalized chainを正しく返すこと
- 監視が5分以内検知、15分以内担当確認、60分以内のBase/IC双方pauseを実証できること

EVM RPC Canister配下providerの運営主体、基盤、可用性は監査対象外であり、production承認条件には含めない。

## 見直し手順

1. Mint limitとwindow長は既存contract上で変更しない。
2. 異なる値が必要な場合は、新contractのdeployを別計画として安全審査する。
3. Service Feeを変更する場合は両方向をpauseし、Ledger feeとの関係を含むreview済みprofileを更新してproduction preflightを再実行する。Ledger feeとService Feeを稼働中に独立変更しない。
