# Plan 003: Settlement Reserve・Runtime Administrator

> **履歴資料**：この本文はPlan 003完了時点の実装境界を記録している。現行実装は明示操作型Settlementを使用する。
> 現行仕様はリポジトリ直下の`README.md`と`docs/`を参照する。

## Status

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plan 001, Plan 002
- **State**: DONE

## Implemented boundary

- ETHとcyclesを別単位で保守的に予約し、非終端Withdrawal数から必要Settlement Reserveをchecked arithmeticで算出する。残高観測不能または不足時はICRC pull前に新規Depositだけを拒否する。
- EVM operationはnonce未割当のQueued intentとしてcalldataを固定し、acknowledgement/refundをmintより先にnonce割当する。Prepared以降はnonce順と同一raw transactionを維持する。
- この当時の複数admin案はPlan 006で置換済みである。現行は単一pause principalが安全操作だけ、SNS GovernanceがFee Recipient、fee payout、再開、role管理を実行する。
- fee payoutはamount、ledger fee、recipient、transfer identityを送信前にstable memoryへ保存する。成功とDuplicateだけでfee reserveを減算し、曖昧結果は履歴照合までHoldする。
- append-only監査ログはpause/resume、rotation、Fee Recipient、fee payout、reserve gate、Base Service Fee観測変更をsequence順に保持する。
- 本番未デプロイ方針に従い現行schema v4だけを受理し、legacy migrationは持たない。

## Verification

- Rust coreはreserve境界、overflow、fee reserve算術、EVM状態順序を検査する。
- PicJSは資産移動saga、pause/resume、reserve不足時のpull未実行、監査ログ、fee payoutをPocketIC上で検査する。
- `scripts/ci-local.sh checks`でRust、Wasm、Candid、PicJS、ICP、Foundry、SMT、Verusを一括検査する。

## Deferred

- 本番reserve数値、鍵の最終保管方式、mainnet deployはPlan 005/006で確定する。
- fee bump、manual Hold resolution、任意transaction送信は導入しない。
