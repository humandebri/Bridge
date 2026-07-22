# Bridge verification ledger

| Claim | Production implementation | Evidence | External assumption |
|---|---|---|---|
| burnと`Committed`化は原子的で、fee drift時はburn前に失敗する | `Bridge.createWithdrawal` | Foundry unit/fuzz/invariant、SMT harness | EVM atomic rollback |
| Committed quoteは`amountOut + chargedServiceFee = amount`で固定される | Base record、`Settlement::validate_committed` | Foundry、Rust、Verus `committed_quote_matches` | canonical Finalized state read |
| BaseにWithdrawal専用のrefund/remint経路がなく、処理済みDeposit IDをreplayできない | `None | Committed` ABI、Deposit deduplication | ABI snapshot、Foundry unit/invariant、SMT | deployed bytecodeが検証対象と一致。Bridge Signerの通常Deposit mint権限は別のtrust assumption |
| Baseへ渡るamount・limit・feeは`u128`境界内で、同一transactionはWithdrawalを一度だけclaimする | `Bridge` production predicate、transient claim | Solidity SMT、Foundry | EIP-1153 transaction lifetime、deployed bytecode一致 |
| Timelock候補はdelay範囲内、closed single-member role、pending operationなしである | `Bridge._validateTimelockCandidate` production predicate | Solidity SMT、Foundry | introspection callとdeployed candidateの真正性 |
| Ledger送金はcanonical FinalizedのCommitted確認後だけ開始する | `notify_withdrawal`、`Observed → ReleasePending` | Rust、integration（Verusはphase遷移のみ） | EVM RPC quorumの真正性 |
| frontendはFinalized成功だけを通知し、revertを破棄する | confirmation coordinator、純粋判断関数 | Leanモデル、網羅Vitest | browser storage、RPC、walletの真正性 |
| 成功・Duplicate・履歴照合成功だけが固定amount・固定IC AccountへのPaidを終端化する | Withdrawal/Reconciliation state machine | Rust、Leanモデル、integration、Verus terminal proof | Ledger履歴の完全性。LeanからRust実装への対応はRust回帰テスト |
| Fee reserveは`chargedServiceFee - actualLedgerFee`を一度だけ計上する | Withdrawal apply/storage transaction | Rust、Verus backing/fee-once proof | SQLite atomic commit |
| stale snapshot workerは新しいrefresh ownerを完了・解放できない | snapshot refresh generation/owner | Rust、Verus production-shared owner/generation proof | SQLite atomic commit、async callback identity |
| Deposit admissionはcounterまたはobservation generation driftを検出する | reserve observation token | Rust、Verus production-shared token proof | SQLite read snapshot |
| settlement lease generationはwrapせず、古いleaseは新しいleaseを完了できない | settlement job claim/finish | Rust、Verus generation proof | SQL row selectionとconditional update |
| Ledger Fee超過時はreleaseを作らず、同じObserved recordを停止・再検証する | `notify_withdrawal`、`continue_withdrawal`、runtime guard | Rust、integration、runtime validation | Ledger fee queryの真正性 |
| WithdrawalはEVM署名・nonce・追加Base transactionを生成しない | adapter operation routing | Rust、integration、ABI selector test | Canister Wasmが検証対象と一致 |
| serialized frontend queue更新は別settlementを失わず、restoreでblockedを解除せず、durable write失敗時もsession copyを保持する | pending confirmation純粋更新 | Leanモデル、網羅Vitest | Web LocksとlocalStorage意味論 |

EVM RPC provider共謀、各providerの`finalized`意味論、browser storage、wallet、Ledger実装、鍵管理、運用補充、未決済債務のlivenessは証明範囲外である。runtime fee guard検出後のBase withdrawal pause、fee同期、再開承認は運用仮定に置く。
