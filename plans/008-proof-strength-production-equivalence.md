# Plan 008: Strengthen formal verification evidence toward production equivalence

## Status

- **Priority**: P1
- **Risk**: MEDIUM
- **Depends on**: Plans 001–004 (verification infrastructure) and current authoritative `verification/` records.
- **State**: COMPLETE (2026-08-18)

## Goal

Preserve the production gate—clean checkout, matching fingerprint, nine passing stages, and matching SHA-256 across two offline builds—while strengthening each claim's weakest evidence by one level. External assumptions including `runtime_toolchain` (TCB) cannot be removed by proof, so accept all claims remaining `partial` by design while improving practical evidence strength in `implementation`, `smt_scalar`, `vector_consumer`, and refinement coverage.

**Exclude** Phase 4 architectural reduction of external assumptions, such as querying `ledger_fee_immutability` at runtime, because it changes safety design.

## Phases

### Phase 0 — Document production-equivalence criteria

- Add a production-equivalence definition section to `verification/README.md`.
- Production equivalence means the release proof gate: clean checkout, matching fingerprint, nine passing stages, and matching SHA-256 across two offline builds.
- State that all claims having `implementation-proved: 0` is intentional because `runtime_toolchain` (TCB) cannot be eliminated.

### Phase 1 — Extend Verus predicate proofs

Promote claims with `implementation: unproved` and concrete Rust implementations to Verus `shared` predicate proofs: `_spec`, proof, and negative fixture sharing expression macros with production. `shared` does not prove entire executable functions; it shares expressions between Cargo and Verus specifications to prevent drift (see `verification/README.md`).

Targets:

| Claim | Target symbol | Work | Result |
|---|---|---|---|
| `withdrawal_admission_boundary` | `kernel.rs#withdrawal_id_is_admissible` | Extract expression macro; add `_spec`, proof, and negative fixture | Complete; promoted to `implementation-proved` |
| `activation_preflight` | `base_governance.rs` preflight/postcondition predicate | Share kernel and include in Verus `shared` coverage | Complete; promoted to `implementation-proved` |
| `governance_nonce_chain_binding` | `evm_rpc.rs#transaction_count` chain binding | Add Verus coverage while retaining `rpc_provider_chain_configuration` | Confirmed out of scope. Chain binding consists of async-boundary assignments in `client()` (`chain_id: args.base_chain_id`, evm_rpc.rs:304) and envelope construction in `prepare` (base_governance.rs:255); no pure decision predicate exists. A Verus specification would be cosmetic proof and is omitted under AGENTS.md. Retain `partial` under `rpc_provider_chain_configuration` |

- Five liveness properties: add the common-enable-condition plus valid-step portion of `occurrence_produces_valid_step` if amenable to `shared` verification. Whole-scheduler fairness remains the `scheduler_weak_fairness` assumption.
- `withdrawal_finalization` / `pending_queue` (pure TypeScript functions): outside Verus scope; cover through Phase 3 vector expansion.
- Update `verification/verus/manifest.tsv`, `verification/verus/pass.rs`, `verification/verus/fail/*.rs`, and `verification/claims.tsv` (`verus_obligations` / `production_links`).
- CI automatically verifies manifest consistency, proof escapes, and 1:1 fixtures; omissions fail closed.

### Phase 2 — Add Solidity SMT obligations

- Current SMT pass files: `MintAuthorizationState.sol`, `WithdrawalState.sol`, `BoundedValue.sol`, and `BridgeAdministrationState.sol` (seven claims).
- Turn boundary predicates in `contracts/src/Bridge.sol`, `MintAccounting.sol`, and `DeploymentPolicy.sol` into SMT harnesses with `assert`.
- Add `verification/smt/pass/*.sol`; register in `claims.tsv` `smt_obligations` and `check_claim_manifest.py` `REQUIRED_SCALAR_CALLS`.

#### Phase 2 results (wire only exact correspondences)

- Of four SMT pass files, `WithdrawalState.sol`, `BoundedValue.sol`, and `BridgeAdministrationState.sol` were verified by forge build but orphaned, without `_obligations` in any claim.
- Compared each orphaned harness assertion with claim abstract theorems and wired only exact matches:
  - `BoundedValue.sol#netAmount` / `#consumeWindow` → `deposit_admission` (abstract: `net = grossAmount - serviceFee ∧ net > 0`, `mintedInWindow + net ≤ mintWindowLimit`). Promoted `smt_scalar` to `implementation-proved`.
  - `BridgeAdministrationState.sol#boundedServiceFee` → `service_fee_maximum` (abstract: `serviceFee ≤ maximumServiceFee`; production `Bridge.sol#setServiceFee` uses `serviceFeeIsValid`). Promoted `smt_scalar` to `implementation-proved`.
- Left nonexact matches unwired to avoid cosmetic proofs: `WithdrawalState.sol#commit` (Solidity Withdrawal commit arithmetic without a corresponding abstract theorem) and other `BridgeAdministrationState.sol` functions (role separation, u128, and Timelock boundaries without matching claim abstracts).
- `REQUIRED_SCALAR_CALLS` validates Bridge mint wrapper refinement, so add no new Withdrawal/Governance scalar calls; requiring functions not called by the production `Bridge.sol` mint wrapper would create false negatives.

### Phase 3 — Expand Lean refinement vector coverage

- Add cases to `verification/lean/BridgeSpec/Vectors.lean`: boundaries, all phase-transition combinations, and rejection paths.
- Extend `refinement-manifest.tsv` sections and add consumers.
- Strengthen coverage of pure TypeScript `withdrawal_finalization` / `pending_queue` here.
- These verification-material changes do not alter safety decisions, but check expanded fingerprint scope in `proof-impact.tsv`.

#### Phase 3 results

- Expanded `Vectors.lean` `finalization_cases` from six to 18 and `queue_cases` from three to 12. Boundaries cover `receiptBlock=0`/`max`, `finalizedBlock=0`/`max`, and all `finalized < receiptBlock`/`≥`/`>` branches. Queue cases cover meaningful combinations of three `existing_blocked` values × two `incoming_blocked` values × two `other_blocked` values.
- Regenerated `verification/generated/protocol-vectors.json` with `python3 scripts/protocol_vectors.py --update`.
- Vitest consumers for finalization/queue in `generated-refinement.test.ts` still PASS; the Rust vector schema consumer also PASSes.
- Existing `refinement-manifest.tsv` sections/consumers suffice; no new sections needed.
- Note: parsing Vitest JSON fails if pnpm engine warnings enter stdout without `fnm exec` using `.node-version` (24.14.0). `scripts/ci-local.sh` selects the correct Node through fnm, so this does not affect the gate.

### Phase 5 — Harden production gate operations

- Verify and document that pre-irreversible-operation hooks such as `production-release.sh` run `scripts/ci-local.sh proofs` from a clean checkout.
- Document Git tracking and enforced fingerprint equality for `verification/output/proof-receipt.json` in the README.

## Verification

After each phase's changes, all of the following must pass:

- `python3 scripts/check_proof_impact.py`
- `python3 scripts/check_claim_manifest.py`
- `scripts/ci-local.sh proofs`
- Applicable unit, negative, refinement, and transaction tests.

## Deferred

- Phase 4: reducing assumptions such as `ledger_fee_immutability` and `eventual_keeper_action` changes safety design and requires separate approval.
- `runtime_toolchain` (TCB), cryptography, RPC authenticity, SQLite atomicity, and browser/wallet boundaries are external assumptions that proofs cannot eliminate; affected claims intentionally remain `partial`.