# Plan 005: KINIC本番パラメータ・emergency pause運用実証

## Status

- **State**: IN PROGRESS
- **Initial activation evidence**: exact schedule／execute gas estimate、10件以上の異なるFinalized fee block、idle cycles burn、実pause principal、固定limit、pause/cancel経路演習
- **Post-unpause Gate C evidence**: 7日以上のBase fee分布とgovernance gas／settlement cycles各10件以上。観測値を運用設定へ自動反映しない。

## Implemented locally

- KINIC Ledger/Index/Root/Governanceの一次証跡と確定済みfee設定
- 保守的parameter derivationとdeployment profile検証CLI
- deploy後に変更不能なMint limitとCanister由来Governance OperatorのTimelock構成
- Bridgeのrequest-time reserve gate、Safe観測時刻、手動pause API
- threshold signer補充、単一emergency pause principalの監視演習とrunbook

初回activation evidenceとpause/cancel演習が欠ける間はmainnet candidateを`validated`にしない。unpause後のGate C evidenceは運用評価として別に収集する。release approver、finance principal、複数pause principal、人間のEVM管理鍵は要求しない。

初期cycles値はpause状態の`idle_cycles_burned_per_day`と固定settlement ceilingから次式で求める。

```text
settlement cycle ceiling = 5,000,000,000 cycles
cycles floor = (idle cycles burn/day + 5,000,000,000) × 30 × 2
```

初期fee証跡はexact schedule／execute calldataのestimateと10件以上の異なるFinalized blockを使い、gas limitを最大estimateの130%から1,000単位で切り上げ、priority feeをp95×4、max feeをbase fee p99×20、L1 ceilingをp99×10とする。quote validityは90秒、13,000／60,000／15,000 bps multiplierは固定する。fee cap超過またはcycles不足ならtransactionを生成・送信しない。Mint Throughput LimitとPer-Deposit Limitはderiveせず、監視5/15/60目標と許容最大被害額に基づく承認済みraw値をprofileへ固定する。pause/cancel演習は経路成功を本番ゲートとし、5/15/60の達成は公開後の運用評価とする。
