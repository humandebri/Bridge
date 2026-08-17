# Plan 008: 形式検証の証拠強度向上（本番相当化）

## Status

- **Priority**: P1
- **Risk**: MEDIUM
- **Depends on**: Plan 001〜004（検証基盤）、現行の`verification/`正本
- **State**: IN PROGRESS

## Goal

本番ゲート（clean checkout + fingerprint一致 + 9 stage pass + offline二回build SHA-256一致）は維持したまま、各claimの最弱証拠を1段階引き上げる。`runtime_toolchain`（TCB）を含む外部仮定は証明で除去できないため、全claimが`partial`であることは設計として受容し、証拠強度（`implementation`、`smt_scalar`、`vector_consumer`、refinement網羅性）を現実的に向上させる。

Phase 4（外部仮定の設計的削減、例: `ledger_fee_immutability`のruntime照会化）は安全設計変更を伴うため**含めない**。

## Phases

### Phase 0 — 本番相当の定義を文書化

- `verification/README.md`に「本番相当の定義」節を追加する。
- 本番相当 = release proof gate（clean checkout + fingerprint一致 + 9 stage pass + offline二回build SHA-256一致）。
- 全claimが`implementation-proved: 0`なのは設計であり、`runtime_toolchain`（TCB）が不可除去であるためと明記する。

### Phase 1 — Verus executable拡張

`implementation: unproved` かつ Rust側に実体があるclaimを、executable proof（実production実行関数を直接呼ぶproof）へ昇格させる。

対象:

| Claim | 対象symbol | 作業 | 結果 |
|---|---|---|---|
| `withdrawal_admission_boundary` | `kernel.rs#withdrawal_id_is_admissible` | `_spec` + executable proof + negative fixture追加 | 完了（`implementation-proved`昇格） |
| `activation_preflight` | `base_governance.rs` preflight/postcondition predicate | kernel共有化 → Verus対象へ | 完了（`implementation-proved`昇格） |
| `governance_nonce_chain_binding` | `evm_rpc.rs#transaction_count` chain binding | Verus対象へ（`rpc_provider_chain_configuration`仮定は残す） | 対象外に確定。chain bindingは`client()`の`chain_id: args.base_chain_id`（evm_rpc.rs:304）と`prepare`のenvelope構築（base_governance.rs:255）というasync境界の代入で、純粋な判定predicateが存在しない。Verus spec化はcosmetic proofになるためAGENTS.md方針により実施しない。`rpc_provider_chain_configuration`仮定で防護される挙動として`partial`を維持 |

- liveness 5件: `occurrence_produces_valid_step`の「共通enable条件 + valid step」部分がexecutable検証可能なら追加。scheduler全体の公平性は`scheduler_weak_fairness`仮定として残す。
- `withdrawal_finalization` / `pending_queue`（TS純粋関数）: Verus対象外。Phase 3のvector拡張で対応する。
- 更新ファイル: `verification/verus/manifest.tsv`、`verification/verus/pass.rs`、`verification/verus/fail/*.rs`、`verification/claims.tsv`（verus_obligations / production_links）。
- CIがmanifest整合・proof escape・fixture 1:1を自動検証するため、追加漏れはfail closedになる。

### Phase 2 — Solidity SMT義務の追加

- 現状SMT pass: `MintAuthorizationState.sol` / `WithdrawalState.sol` / `BoundedValue.sol` / `BridgeAdministrationState.sol`（7 claim分）。
- `contracts/src/Bridge.sol`・`MintAccounting.sol`・`DeploymentPolicy.sol`の境界predicateをSMT harness（`assert`）化する。
- `verification/smt/pass/*.sol`追加、`claims.tsv`の`smt_obligations`と`check_claim_manifest.py`の`REQUIRED_SCALAR_CALLS`に反映する。

#### Phase 2 実施結果（厳密対応のみwiring）

- SMT pass 4ファイルのうち`WithdrawalState.sol`・`BoundedValue.sol`・`BridgeAdministrationState.sol`はforge buildで検証されるが、どのclaimにも`_obligations`が無い（orphaned）状態だった。
- 各orphaned harness関数のassertとclaimのabstract theoremを照合し、厳密一致するもののみwiring:
  - `BoundedValue.sol#netAmount` / `#consumeWindow` → `deposit_admission`（abstract: `net = grossAmount - serviceFee ∧ net > 0`、`mintedInWindow + net ≤ mintWindowLimit`）。`smt_scalar`が`implementation-proved`に昇格。
  - `BridgeAdministrationState.sol#boundedServiceFee` → `service_fee_maximum`（abstract: `serviceFee ≤ maximumServiceFee`、production `Bridge.sol#setServiceFee` が`serviceFeeIsValid`を使用）。`smt_scalar`が`implementation-proved`に昇格。
- 厳密一致しないものはcosmetic proof回避のためwiringしない: `WithdrawalState.sol#commit`（Solidity側withdrawal commit算術。対応するabstract theoremが存在しない）、`BridgeAdministrationState.sol`の他関数（role分離・u128・timelock境界。該当claim abstractなし）。
- `REQUIRED_SCALAR_CALLS`はBridge mint wrapperのwrapper refinement検証用のため、withdrawal/governance側の新規scalar呼び出しは追加しない（production `Bridge.sol` mint wrapperが呼ばない関数を必須化すると偽陰性になるため）。

### Phase 3 — Lean refinement vector網羅性拡張

- `verification/lean/BridgeSpec/Vectors.lean`のcase追加（境界値、位相遷移の全組合せ、reject系）。
- `refinement-manifest.tsv`のsection拡張とconsumer追加。
- `withdrawal_finalization` / `pending_queue`のTS純粋関数はここで網羅性を強化する。
- 検証資材の変更なので安全判断は伴わないが、fingerprint範囲が拡がる点を`proof-impact.tsv`で確認する。

### Phase 5 — 本番ゲート運用堅牢化

- `production-release.sh`等の不可逆操作直前hookがclean checkoutで`scripts/ci-local.sh proofs`を実行することを確認・文書化する。
- `verification/output/proof-receipt.json`のgit追跡・指紋一致強制をREADMEに明記する。

## Verification

各Phaseの変更後、以下が全てpassすること:

- `python3 scripts/check_proof_impact.py`
- `python3 scripts/check_claim_manifest.py`
- `scripts/ci-local.sh proofs`
- 関連するunit / negative / refinement / transaction test

## Deferred

- Phase 4: `ledger_fee_immutability`、`eventual_keeper_action`等の外部仮定の設計的削減は、安全設計変更を伴うため別途承認後に実施する。
- `runtime_toolchain`（TCB）、暗号、RPC真正性、SQLite atomicity、ブラウザ/ウォレット境界は証明で除去できない外部仮定であり、該当claimは意図的に`partial`のまま維持する。