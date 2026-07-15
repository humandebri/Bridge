# Plan 005: KINIC本番パラメータ・鍵管理・Base Sepolia運用実証

## Status

- **State**: IN PROGRESS
- **Blocked completion evidence**: Sepolia各100回gas/cycles計測、Base mainnet 30日fee分布、実Base Admin/Runtime/finance/pause principal、固定limitの承認、7日運転記録

## Implemented locally

- KINIC Ledger/Index/Root/Governanceの一次証跡と確定済みfee設定
- 保守的parameter derivationとdeployment profile検証CLI
- deploy後に変更不能なMint limitと単一Base Admin walletのTimelock構成
- Bridgeのrequest-time reserve gate、Safe観測時刻、手動pause API
- 鍵ceremony、補充、緊急pause runbook

外部証跡とceremonyが欠ける間はmainnet candidateを`validated`にせず、Plan 005を完了へ更新しない。
