# 実装計画

この索引はBase contractのPhase 1E完了時点から始まった実装計画と現在の進捗を記録する。
Plan 001〜004は完了済みの履歴資料であり、現行仕様はリポジトリ直下の`README.md`と`docs/`を正本とする。

## 実行順序

| Plan | 内容 | 優先度 | 規模 | 依存 | 状態 |
|---|---|---:|---:|---|---|
| [001](001-phase2-deterministic-state-machine.md) | Phase 2時点の決定的状態機械、stable schema、read-only Candid境界 | P1 | L | — | DONE |
| [002](002-phase3-external-integrations.md) | ICRC ledger / EVM RPC / threshold ECDSAの外部連携とReconciliation Hold | P1 | L | 001 | DONE |
| [003](003-settlement-reserve-runtime-admin.md) | Settlement Reserve、Runtime Administrator、運用監査ログ | P1 | L | 001, 002 | DONE |
| [004](004-plan003-asset-safety-verus.md) | Verusでcanister coreとcross-system境界の証明を追加 | P1 | L | 001, 002, 003 | DONE |
| [005](005-production-parameters-key-operations.md) | 対象SNS・数値パラメータ・鍵管理・testnet運用の確定 | P1 | M | 001〜004 | IN PROGRESS |
| [006](006-sns-handover-upgrade-production-preflight.md) | SNS handover、upgrade互換性、production preflight | P0 | L | 001〜005 | IN PROGRESS |
| [007](007-local-ic-mainnet-base-sepolia-frontend-e2e.md) | LocalからIC mainnet test Canister・Base Sepolia・test frontendへのE2E | P0 | L | 001〜004 | LOCAL DONE / EXTERNAL PENDING |

## 依存関係

- 001で状態・ID・冪等性・stable schemaを固定しない限り、外部呼び出しを実装しない。
- 002は001のpure coreを呼び出すadapterとして作り、ICRC/EVMの失敗をcoreの状態遷移へ変換する。
- 003はSettlementの実コストと未完了状態を観測できる002の後に実装する。ただし権限モデルと監査ログの設計は001と並行してレビューできる。
- 004はproductionと共有するcoreが存在してから、各proof obligationを追加する。
- 005のTBD解消は、初期値を本番へ入れる前に必要であり、006のpreflightをブロックする。
- 007はproduction/SNSから独立したstaging検証である。local gateのclean commit証跡なしに外部stageへ進めない。

## 今回の推奨着手点

Plan 007のlocal gateをclean commitから再実行してpromotion evidenceを発行し、明示承認後にIC mainnet test CanisterとBase Sepoliaの外部stageへ進む。並行してPlan 005の外部計測を完了する。

## 残作業の完了条件

production開始には、Base contractの検証成功だけでは足りない。少なくとも、001〜007、`docs/parameters.md`のTBD解消、対象SNSの確定、SNS Root単独controllerへのhandover、testnetでのdeposit・withdrawal release・Ledger fee guard・reconciliation・upgrade実証、鍵と監視の運用runbookが必要である。
