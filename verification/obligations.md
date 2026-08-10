# Bridge verification ledger

Leanを抽象protocol specificationの正本とし、生成vectorでproduction consumerとのbounded conformanceを検査する。
vectorに列挙されない入力や副作用を含む実装全体のsemantic refinementは主張しない。

Evidenceはclaimごとに、抽象Lean proof、有限幅Lean refinement、Verus evidence、production transaction test、外部仮定を独立に読む。Verus manifestの`executable`はproduction実行関数を直接呼ぶproof、`shared`はCargo式とspecが式macroを共有するpredicate proof、`model`はproduction symbolを持たないモデルproofであり、三者を同じ強度として扱わない。
release対象の閉じたclaim集合、trace theorem、Verus obligation、production symbol、transaction selector、仮定IDは`claims.tsv`を正本とし、表の説明だけでは完了判定しない。statusはproof gateが証拠の最弱要素から算出する。
外部仮定の依存claim、検査可能なfault test、運用監視、破壊時のfail-closed動作は`assumptions.tsv`を正本とする。

| Claim | Production implementation | Evidence | External assumption |
|---|---|---|---|
| burnと`Committed`化は原子的で、fee drift時はburn前に失敗する | `Bridge.createWithdrawal` | Foundry unit/fuzz/invariant、SMT harness | EVM atomic rollback |
| Committed quoteは`amountOut + chargedServiceFee = amount`で固定される | Base record、`Settlement::validate_committed` | Lean定理、生成vector、Foundry consumer、Rust consumer、Verus `committed_quote_matches` | canonical Finalized state read |
| BaseにWithdrawal専用のrefund/remint経路がなく、処理済みDeposit IDをreplayできない | `None | Committed` ABI、Deposit deduplication | ABI snapshot、Foundry unit/invariant、SMT | deployed bytecodeが検証対象と一致 |
| Mint Authorizationはchain、contract、全Mint field、deadline、epochへ束縛され、任意callerはrecipientを変更できない | `Bridge.mintDepositWithAuthorization`、Rust/TS EIP-712 digest | 共有protocol vector、Foundry unit/fuzz、Rust、Vitest | threshold ECDSA鍵とwallet RPCの真正性 |
| 同じDepositには一つのAuthorization digestだけを保存し、再署名でdeadlineを変更しない | `MintAuthorizationRecord`、署名dispatch transaction | Rust state/storage/upgrade test | SQLite atomic commit、async callback identity |
| 返金開始時には受理可能なAuthorizationが残らない | `timestamp > deadline && processed == false`のFinalized証拠 | Rust state/storage/integration、Contract期限境界test | provider quorumのFinalized意味論、EIP-1898 canonicality |
| processedなDepositはexact eventとcanonical receipt証拠なしにMintedにならず、証拠不一致を返金へfallbackしない | `exact_mint_evidence`、`MintFinalizationEvidence` | Rust state/integration、Foundry event digest | provider quorum、receipt/log decoderの真正性 |
| pauseとsigner rotationでepochは単調増加し、旧Authorizationを一括失効する | `mintAuthorizationEpoch` | Foundry unit/invariant、共有EIP-712 vector | EVM execution atomicity |
| Baseへ渡るamount・limit・feeは`u128`境界内で、同一transactionはWithdrawalを一度だけclaimする | `Bridge` production predicate、transient claim | Solidity SMT、Foundry | EIP-1153 transaction lifetime、deployed bytecode一致 |
| Timelock候補はdelay範囲内、closed single-member role、pending operationなしである | `Bridge._validateTimelockCandidate` production predicate | Solidity SMT、Foundry | introspection callとdeployed candidateの真正性 |
| Ledger送金はcanonical FinalizedのCommitted確認後だけ開始する | `notify_withdrawal`、`Observed → ReleasePending` | Rust、integration（Verusはphase遷移のみ） | EVM RPC quorumの真正性 |
| frontendはWithdrawalのFinalized成功だけを通知し、revertを破棄する | Withdrawal confirmation coordinator、純粋判断関数 | Lean定理、生成vector、TypeScript consumer、Vitest | browser storage、RPC、walletの真正性 |
| 成功・Duplicate・履歴照合成功だけが固定amount・固定IC AccountへのPaidを終端化する | Withdrawal/Reconciliation state machine | Lean定理、Rust state/integration test、Verus terminal proof | Ledger履歴の完全性。LeanからRustへの対応はRust回帰テスト |
| Fee reserveは`chargedServiceFee - actualLedgerFee`を一度だけ計上する | Withdrawal apply/storage transaction | Lean raw-transition保存、Rust transaction test、Verus settlement executable proofとfee-once predicate proof | SQLite atomic commit |
| stale snapshot workerは新しいrefresh ownerを完了・解放できない | snapshot refresh generation/owner | Rust、Verus production-shared owner/generation proof | SQLite atomic commit、async callback identity |
| DepositはLedger pull後にFinalized quoteを確定し、quote transactionがmint capacityとobservation generation driftを再検証する | funding attempt、quote、reserve observation token | Rust storage/integration、生成vector、Verus production-shared predicate proof | SQLite transaction atomicity、Finalized snapshotの真正性 |
| Deposit refundは`refund amount + 10_000 = gross amount`を維持し、Service Feeを計上しない | Deposit refund identity、Deposit fee delta | Rust state/storage/integration、Verus refund arithmetic・fee-once proof | 固定Ledger fee、Ledger履歴の完全性 |
| Refund holdは成功証拠または完全な不存在証明なしに新attemptへ進まない | RefundReconciliationHold、exact transfer evidence | Rust reconciliation test、Verus evidence binding・hold phase proof | Ledger/Index/archive scanの完全性 |
| settlement lease generationはwrapせず、古いleaseは新しいleaseを完了できない | settlement job claim/finish | Rust、Verus generation proof | SQL row selectionとconditional update |
| Authorization期限照合の手動claimはjobを早く起こせるだけで、Base Finalized期限判定を迂回できない | stable settlement executor manual claim、expiry reconciliation | Rust state/storage/integration、Verus共有predicate | SQLite atomic commit、async callback identity |
| 固定Ledger Feeがcharged Service Feeを超える場合はreleaseを作らず、同じObserved recordを停止・再検証する | `notify_withdrawal`、`continue_withdrawal`、固定fee guard | Rust、integration、runtime validation | Ledger Feeが100,000 rawで不変という外部仮定 |
| WithdrawalはEVM署名・nonce・追加Base transactionを生成しない | adapter operation routing | Rust、integration、ABI selector test | Canister Wasmが検証対象と一致 |
| serialized Withdrawal通知queue更新は別settlementを失わず、restoreでblockedを解除せず、durable write失敗時もsession copyを保持する | Withdrawal pending confirmation純粋更新 | Lean定理、生成vector、TypeScript consumer、Vitest | Web LocksとlocalStorage意味論 |

EVM RPC provider共謀、各providerの`finalized`意味論、browser storage、wallet、Ledger実装、固定Ledger Fee、鍵管理、運用補充、未決済債務のlivenessは証明範囲外である。固定fee guard検出後のBase withdrawal pause、設定確認、再開承認は運用仮定に置く。runtime settlementは`icrc1_fee()`を照会しない。
