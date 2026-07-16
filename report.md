# KINIC–Base Bridge セキュリティ検証レポート

レビュー日：2026-07-16
対象：staged、unstaged、untrackedを含む現在の作業ツリー
前提：本番未デプロイのため、旧ABI、旧Candid、旧stable schemaとの互換処理は持たない

## 結論

WithdrawalをBase上の不可逆な`Committed` burnとCanister上の未決済債務へ置換した。Base refund、release acknowledgement、Withdrawal用EVM operation、nonce、threshold ECDSA署名、Withdrawal用EVM confirmation jobは現行sourceに存在しない。Depositの`MintDeposit`だけがEVM operationとconfirmation jobを持つ。

Contractはburn前にService Fee上限と`amount > serviceFee`を検証し、`amountOut = amount - chargedServiceFee`を固定する。Canisterはreceipt、event、state、Bridge signer、runtimeを同じ2-of-3 quorum Finalized block hashへ束縛し、完全一致後にだけ固定IC Accountへ固定額を送る。Ledger FeeはBridge負担で、Fee reserveには`chargedServiceFee - actualLedgerFee`だけを計上する。

Deposit受付snapshot、Mint成功・revert、GovernanceによるMint revert救済もFinalized基準とした。Finalized未対応、head不一致、hash不一致ではSafeへfallbackせずfail closedとする。Pending nonceはPending、現在ETH残高はSafeを維持し、ReserveにはSafe残高とFinalized block残高の小さい方を使う。

## 検証結果

| 対象 | 判定 | 根拠 |
|---|---|---|
| atomic burn・Committed quote | PASS | Foundry unit/fuzz/invariant、selector fixture |
| Withdrawal状態機械・会計 | PASS | Rust workspace tests、Verus shared kernel |
| 追加Base transaction不在 | PASS | adapter/integration test、EVM kind縮小、ABI selector test |
| UIのFinalized待機・fee再検証 | PASS | Vitest 83件、typecheck、lint |
| Stable state | PASS | stable schemaはv10だけ、wire v9だけを受理。current-schema reopen、未知version fail closed、旧migrationなし |
| 形式モデル | PASS | Lean、Verus、Solidity SMT pass/fail fixture |
| 公式EVM RPC Canister実演習 | PENDING EXTERNAL RETEST | rehearsalとvalidatorは存在するが実ネットワーク未実施 |

## 残存リスク

- providerの`finalized`意味論と2-of-3 quorumの正しさは外部仮定であり、Base Sepolia本番候補RPC 3社でのrehearsalが必要である。
- burn後にBase refundはない。Ledger停止、Canister停止、資金不足は同じWithdrawal ID・IC Account・transfer identityによる再試行、履歴照合、運用補充で解消する必要がある。
- `actualLedgerFee <= chargedServiceFee`は運用前提である。違反時は送金前停止となり、利用者受取額の減額や送金先変更は行わない。
- EVM RPC quorum、ICRC Ledger履歴、SQLite atomicity、鍵管理、監視応答は機械証明の外部仮定である。

本番deploy、controller handover、unpause、資産受付開始には、全CI、外部rehearsal、署名済みGate B evidence bundleと別の明示承認が必要である。
