# パラメータ導出

Bridge の安全パラメータの導出式と初期値を記録する。
すべて raw unit で定義し、token decimals の表示変換を判断に使わない（ADR 0001）。
値の変更は ADR 0009 の権限分割に従う。引き下げは Runtime Administrator、引き上げは Base Admin の timelock 経由とする。

## Mint Throughput Limit

fixed window（ADR 0012）で実装するため、window 境界をまたぐ短時間に最大で上限の 2 倍がmintされうる。

```
window あたりの上限 = 許容最大被害額 ÷ 2
許容最大被害額 = 監視が pause を発動するまでの想定時間内に失ってよい額
```

- window 長の初期値: 1 時間
- 上限値: TBD（対象 SNS トークンの流通量と監視体制の確定後に決定）

window を 1 時間とする場合、監視は 1 時間以内の pause 発動を前提とする。
監視体制がこれを満たせないなら、window を延ばすのではなく上限値を下げる。

## Per-Deposit Limit

```
Per-Deposit Limit = 流通中 Bridgeable SNS Token の 1〜5%
```

- 初期値: TBD（対象 SNS トークン確定後に決定）

Mint Throughput Limit が総量を抑えるため、この値は単発の入力ミスと単一要求の異常検知を目的とする。

## MAX_SERVICE_FEE

immutable であり、デプロイ後にいかなる権限でも変更できない（ADR 0004）。
将来の価格変動に耐えるよう保守的に高く置き、運用値の `service_fee` はその遥か下から始める。

```
MAX_SERVICE_FEE = SNS ledger fee × 100〜1000
```

- 初期値: TBD（対象 SNS トークンの ledger fee 確定後に決定）
- `service_fee` 運用初期値: TBD

## Settlement Reserve

固定 floor に未完了 Settlement の保守的最大費用を加算する（ADR 0005）。

```
必要 Settlement Reserve =
    固定 floor
  + Σ (未完了 Settlement ごとの gas limit × max fee per gas 上限)
  + cycles の N 日分の運用費
```

- 固定 floor: TBD
- max fee per gas 上限: TBD（外部仮定として監査対象）
- N: TBD（初期案 30 日）

## timelock 遅延（Base Admin）

- 初期値: 72 時間（ADR 0009）
- 短縮は timelock 自身を経由する。

## 外部仮定の監査リスト

以下は Bridge 内部で保証できず、値の妥当性を運用監査で維持する（ADR 0005、0011）。

- gas 価格の上限評価
- EVM RPC 費用と management canister call 費用の上限評価
- EVM RPC provider 3 中 2 の合意が誤らないこと
- 監視が Mint Throughput Limit の window 長以内に pause を発動できること

## 見直し手順

1. 値の変更提案は本文書の式に対する入力値の変更として記述する。
2. 引き上げ方向は Base Admin の timelock を経由し、待機期間中に公開する。
3. 変更後の値で ADR 0001 と 0005 の証明前提が保たれることを確認する。
