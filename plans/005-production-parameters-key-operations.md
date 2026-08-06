# Plan 005: KINIC本番パラメータ・emergency pause運用実証

## Status

- **State**: IN PROGRESS
- **Blocked completion evidence**: Sepoliaのgovernance gasとsettlement cycles各10回、7日以上のBase fee分布、承認済み日次settlement上限、実pause principal、固定limitの承認、pause/cancel経路演習

## Implemented locally

- KINIC Ledger/Index/Root/Governanceの一次証跡と確定済みfee設定
- 保守的parameter derivationとdeployment profile検証CLI
- deploy後に変更不能なMint limitとCanister由来Governance OperatorのTimelock構成
- Bridgeのrequest-time reserve gate、Safe観測時刻、手動pause API
- threshold signer補充、単一emergency pause principalの監視演習とrunbook

外部証跡とpause/cancel演習が欠ける間はmainnet candidateを`validated`にせず、Plan 005を完了へ更新しない。release approver、finance principal、複数pause principal、人間のEVM管理鍵は要求しない。

cycles floorはpause状態の`baseline_cycles_per_day`、10回計測の`settlement_cycles`最大値、承認済み`expected_daily_settlements`から次式で求める。

```text
cycles floor = (baseline cycles/day + max(settlement cycles) × settlements/day) × 30 × 2
settlement cycle ceiling = ceil(max(settlement cycles) × 1.5)
```

Base fee証跡は開始・終了時刻で7日以上を検証し、過去blockから取得してよい。Mint Throughput LimitとPer-Deposit Limitはderiveせず、監視5/15/60目標と許容最大被害額に基づく承認済みraw値をprofileへ固定する。pause/cancel演習は経路成功を本番ゲートとし、5/15/60の達成は公開後の運用評価とする。
