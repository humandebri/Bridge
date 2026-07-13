# Deployment profiles

`bridge-profile`は秘密を含まないJSON profileと計測evidenceを検査する。

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
```

`derive`はgasとcyclesを各operation 100件、Base feeの30日sampleが揃わなければ失敗する。Mint limitとwindow長はderiveせず、profileへraw unitで明示する。`validate`はmainnetのcanonical KINIC IDs、Sepoliaのtest-only ledger、単一Base Admin wallet、72時間Timelock、固定limit、role分離、3 RPC、fee/reserve関係を検査し、canonical profile SHA-256を出力する。

credential、seed、private key、hardware wallet backup、credential入りRPC URLはprofileへ記録しない。実profileはceremonyで公開値が揃ってから追加し、`status: "validated"`へ変更する前後のhashをレビューする。

## Base Sepolia contract-only experiment

[`scripts/base-sepolia-experiment/`](../scripts/base-sepolia-experiment/)は、固定limit版Bridgeと72時間Timelockの実transaction検証を段階実行する。
再開手順と秘密情報の扱いは[`docs/runbooks/base-sepolia-rehearsal.md`](../docs/runbooks/base-sepolia-rehearsal.md)に記録する。

作業中の公開manifestは[`base-sepolia-contract-experiment.json`](base-sepolia-contract-experiment.json)であり、スクリプトがstate、transaction、finalityを更新する。
各回の公開スナップショットは`deployments/base-sepolia/YYYY-MM-DD/manifest.json`へ保存し、未実行項目を推測値で埋めない。
2026年7月13日の記録は[`base-sepolia/2026-07-13/manifest.json`](base-sepolia/2026-07-13/manifest.json)を参照する。

実験用deployerがBase Admin walletとRuntime Administratorを兼任するため、本番role分離の証跡としては使用しない。
private key、seed、keystore password、credential付きRPC URLは保存しない。
