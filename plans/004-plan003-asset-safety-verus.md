# Plan 004: Plan 003の資産安全境界をVerusで証明する

> **履歴資料**：この本文はPlan 004完了時点の証明境界を記録している。現行の証明台帳は`verification/`を正本とする。
> 現行仕様はリポジトリ直下の`README.md`と`docs/`を参照する。

## Status

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plan 001, Plan 002, Plan 003
- **State**: DONE

## Implemented boundary

- reserve、nonce、fee payout、administrator、audit sequence、EVM rankの判断をallocation・I/Oなしのproduction共有kernelへ集約した。
- 同じ式をVerus specから参照し、境界、単調性、優先順、overflow拒否、role×action許可集合を証明した。
- proof manifestで全資産安全kernelをpass proofと領域別negative fixtureへ対応付け、CIで欠落とproof escapeを拒否する。
- 外部応答とstable/async原子性は信頼境界として残し、Rust、storage reopen、PicJSでcoordinatorとの結合を検査する。

## Verification

- Rust exhaustive testはu128/u64境界と全administrator action×roleを列挙する。
- 各negative fixtureは単独でpostcondition violationになることをCIが最後まで検査する。
- `scripts/ci-local.sh checks`でRust、Wasm、Candid、PicJS、ICP、Foundry、SMT、Verusを一括検査する。

## Deferred

- provider・Ledger・Index・archive応答の真正性、本番reserve値、鍵保管、mainnet deployはPlan 005/006の対象とする。
