# Bridge verification boundary

## Machine-checked claims

- `verification/lean/BridgeModel.lean` models ICP escrow, Base supply, confirmed fee reserve,
  Deposit liability, one target Withdrawal liability, all other Withdrawal liabilities and the
  user-signed `createWithdrawal` transaction that atomically burns bSNS and creates a `Releasing`
  Withdrawal. Lean 4 proves, without `sorry`, that every modeled transition
  preserves 1:1 backing, that one Withdrawal cannot receive both an ICP release and a Base refund,
  and that a reachable economically terminal target Withdrawal has zero target liability without
  changing other Withdrawals' liability.
- `canister/bridge-core/src/kernel.rs` is dual-compiled by Cargo and Verus. The production record
  state machines call its phase, reserve, BadFee, fee-once, terminal-liability residual, counter
  and nonce-conflict kernels.
  `verification/verus/pass.rs` proves their local contracts and algebraic backing refinements.
- `verification/verus/manifest.tsv` classifies every Verus spec as either a production-shared
  kernel or a model-only definition. Each row names its proof, one independent negative fixture,
  and (for shared kernels) the production connection checked by CI.

Run the pinned tools with:

```sh
lean verification/lean/BridgeModel.lean
verus --crate-type bin --no-cheating verification/verus/pass.rs
```

## Explicit refinement boundary

The Lean model does not assert external facts as axioms. `WorldAssumptions`, `TrustedWorld` and
`RefinedExecution` require an honest Bridge signer, the configured EVM RPC Canister quorum returning
the canonical Base Safe chain, authentic Ledger results and atomic IC/SQLite commit as explicit
inputs to the cross-system refinement theorem.
`ValidInitial` constructs terminal-liability safety from an idle, unreceived, zero-target-liability,
backed and nonnegative initial state. These inputs are not proved by Lean. The configured EVM
RPC Canister quorum returning the canonical Safe chain, chain-key signing, ICRC archive
completeness, stable VFS/SQLite semantics, Serde/CBOR,
and IC message rollback remain trusted platform or adapter assumptions. The terminal-liability
claim is a safety property for one modeled Withdrawal; it does not prove that all live Withdrawals
eventually reach a terminal state or that aggregate outstanding Bridge liability becomes zero.
It also does not prove Base finality: a reorg after Safe observation and before finalization is an
explicitly accepted external risk and can break the modeled 1:1 correspondence.
Lean's `releaseIcp` abstracts the Ledger transfer and Safe-confirmed Base acknowledgement into one
economic transition. A post-Safe, pre-finality Base reorg is outside this proof and is an explicitly
accepted risk; this model must not be described as proving L1 finality. For refunds, the production event contains no amount. The operation bundle
therefore checks the Refund kind, operation ID, payload hash, and exact
`refundWithdrawal(uint256)` selector plus Withdrawal ID before persistence. The Base contract uses
that ID to refund its stored gross amount. Verus proves that all four binding predicates are
required, but the Rust/Foundry refinement—not Lean—connects calldata execution to the external
refund amount.

Solidity SMT proves the transition harness and predicates. Bridge storage/calldata integration is
checked by Foundry invariants and ABI drift tests; SMT results must not be described as a proof of
the complete deployed Bridge contract.

## Test-only scope

Canonical receipt/hash queries, same-block Base snapshot reads, SQLite failure injection,
schema v6再オープン / fail-closed behavior, Candid/ABI generation, PocketIC, Anvil, browser behavior and external
Ledger/EVM adapters are refinement tests rather than Lean/Verus theorems. Production monitoring,
key ceremony and operator response are operational evidence and remain deploy blockers until
demonstrated. Provider operator/infrastructure diversity below the EVM RPC Canister is outside the
audit scope and is not a deploy blocker.

The repository is pre-deployment. Only schema v6 with wire v6 is accepted; no legacy migration, compatibility
shim, dual-read path or fallback is part of the verified system.
