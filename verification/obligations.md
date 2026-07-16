# Bridge verification ledger

| Claim | Production implementation | Evidence | External assumption |
|---|---|---|---|
| burnと`Committed`化は原子的で、fee drift時はburn前に失敗する | `Bridge.createWithdrawal` | Foundry unit/fuzz/invariant、SMT harness | EVM atomic rollback |
| Committed quoteは`amountOut + chargedServiceFee = amount`で固定される | Base record、`Settlement::validate_committed` | Foundry、Rust、Verus `committed_quote_matches` | canonical Finalized state read |
| BaseにWithdrawalの再mint経路がない | `None | Committed` ABI | selector test、Foundry invariant、SMT | deployed bytecodeが検証対象と一致 |
| Ledger送金はcanonical FinalizedのCommitted確認後だけ開始する | `notify_withdrawal`、`Observed → ReleasePending` | Rust、integration、Verus phase proof | EVM RPC quorumの真正性 |
| 成功・Duplicate・履歴照合成功だけがPaidを終端化する | Withdrawal/Reconciliation state machine | Rust、integration、Verus terminal proof | Ledger履歴の完全性 |
| Fee reserveは`chargedServiceFee - actualLedgerFee`を一度だけ計上する | Withdrawal apply/storage transaction | Rust、Verus backing/fee-once proof | SQLite atomic commit |
| WithdrawalはEVM署名・nonce・追加Base transactionを生成しない | adapter operation routing | Rust、integration、ABI selector test | Canister Wasmが検証対象と一致 |

EVM RPC provider共謀、各providerの`finalized`意味論、Ledger実装、鍵管理、運用補充、未決済債務のlivenessは証明範囲外である。
