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
- production上限値: 未確定。承認済みdeployment profileへraw unitで明示し、deploy後は変更しない。

監視はwindow長とは別に、異常を5分以内に検知し、15分以内に担当者が確認し、60分以内にBaseとICの双方をpauseする。
監視体制がこれを満たせないなら、window を延ばすのではなく上限値を下げる。

## Per-Deposit Limit

```
Per-Deposit Limit = 承認済みdeployment profileの固定値
```

- production初期値: 未確定。Mint Throughput Limit以下のraw unitをprofileへ明示する。

Mint Throughput Limit が総量を抑えるため、この値は単発の入力ミスと単一要求の異常検知を目的とする。

## MAX_SERVICE_FEE

immutable であり、デプロイ後にいかなる権限でも変更できない（ADR 0004）。
将来の価格変動に耐えるよう保守的に高く置き、運用値の `service_fee` はその遥か下から始める。

```
MAX_SERVICE_FEE = SNS ledger fee × 100〜1000
```

- KINIC ledger fee: `100000` raw
- `MAX_SERVICE_FEE`: `10000000` raw（ledger feeの100倍）
- `service_fee`運用初期値: `1000000` raw（ledger feeの10倍）

## Settlement Reserve

固定 floor に未完了 Settlement の保守的最大費用を加算する（ADR 0005）。

```
必要 Settlement Reserve =
    固定 floor
  + Σ (未完了 Settlement ごとの gas limit × max fee per gas 上限)
  + cycles の N 日分の運用費
```

- ETH固定floor: 未確定。Sepolia gas 100回計測とBase mainnet 30日fee分布の証跡が揃った後、Settlement 100件分を設定する。
- max fee per gas上限: 未確定。Base mainnet直近30日のbase fee p99×2 + priority fee p95で算出する。
- cycles floor: 未確定。7日間の実測から30日分を設定する。
- N: 30日

未確定値をzeroや仮値でmainnet profileへ入れてはならない。`bridge-profile validate-bundle --offline`と`verify-live`は、実artifact、署名、zero reserve、証跡欠落をfail closedで拒否する。

## timelock 遅延（Base Admin）

- 初期値: 72 時間（ADR 0009）
- 短縮は timelock 自身を経由する。

## 外部仮定の監査リスト

以下は Bridge 内部で保証できず、値の妥当性を運用監査で維持する（ADR 0005、0011）。

- gas 価格の上限評価
- EVM RPC 費用と management canister call 費用の上限評価
- 公式EVM RPC Canisterと設定されたquorumがcanonical Safe chainを正しく返すこと
- 監視が5分以内検知、15分以内担当確認、60分以内のBase/IC双方pauseを実証できること

EVM RPC Canister配下providerの運営主体、基盤、可用性は監査対象外であり、production承認条件には含めない。

## 見直し手順

1. Mint limitとwindow長は既存contract上で変更しない。
2. 異なる値が必要な場合は、新contractのdeployを別計画として安全審査する。
3. Service FeeはRuntime Administratorが`MAX_SERVICE_FEE`以内で変更する。
