# 実装計画

この計画は、Base contractのPhase 1E完了時点（`5fc223c`、2026-07-13）を基準にした次の実装順序を記録する。
Phase 2の詳細計画だけを今回作成し、後続Phaseは依存関係が分かる粒度のロードマップとして管理する。

## 実行順序

| Plan | 内容 | 優先度 | 規模 | 依存 | 状態 |
|---|---|---:|---:|---|---|
| [001](001-phase2-deterministic-state-machine.md) | canisterの決定的状態機械、stable schema、read-only Candid境界 | P1 | L | — | TODO |
| 002 | ICRC ledger / EVM RPC / threshold ECDSAの外部連携とReconciliation Hold | P1 | L | 001 | 未作成 |
| 003 | Settlement Reserve scheduler、Runtime Administrator、運用監査ログ | P1 | L | 001, 002 | 未作成 |
| 004 | Verusでcanister coreとcross-system境界の証明を追加 | P1 | L | 001, 002, 003 | 未作成 |
| 005 | 対象SNS・数値パラメータ・鍵管理・testnet運用の確定 | P1 | M | 001〜004と並行 | 未作成 |
| 006 | SNS handover、upgrade互換性、x402 testnet、production preflight | P0 | L | 001〜005 | 未作成 |

## 依存関係

- 001で状態・ID・冪等性・stable schemaを固定しない限り、外部呼び出しを実装しない。
- 002は001のpure coreを呼び出すadapterとして作り、ICRC/EVMの失敗をcoreの状態遷移へ変換する。
- 003はSettlementの実コストと未完了状態を観測できる002の後に実装する。ただし権限モデルと監査ログの設計は001と並行してレビューできる。
- 004はproductionと共有するcoreが存在してから、各proof obligationを追加する。
- 005のTBD解消は、初期値を本番へ入れる前に必要であり、006のpreflightをブロックする。

## 今回の推奨着手点

まず001を実装する。現在のRust coreはPhase 0の空crate（`canister/bridge-core/src/lib.rs`）で、canisterのCandid serviceも空（`canister/bridge-canister/bridge.did`）である。ここに外部I/Oを持ち込まず、状態遷移と永続化の正本を作ることが、以後のledger、EVM、Verus、upgrade検証の共通前提になる。

## 残作業の完了条件

production開始には、Base contractの検証成功だけでは足りない。少なくとも、001〜006、`docs/parameters.md`のTBD解消、対象SNSの確定、SNS Root単独controllerへのhandover、testnetでのdeposit・release・refund・reconciliation・upgrade実証、鍵と監視の運用runbookが必要である。
