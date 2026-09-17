# Plan 001: Implement the Phase 2 deterministic Bridge state machine

> **Historical record:** this document records assumptions and completion criteria at Plan 001 implementation time. The current implementation uses explicit Settlement operations without Canister timers.
> See the repository-root `README.md` and `docs/` for current specifications.

> **Executor instructions:** execute this plan in order, verifying each step before proceeding. If a `STOP condition` applies, stop implementation and report the diff and decision evidence. Update `plans/README.md` when complete.

> **Drift check (run first):** `git diff --stat 5fc223c..HEAD -- Cargo.toml canister/bridge-core canister/bridge-canister scripts/ci-local.sh docs/adr/0008-handover-bridge-upgrades-to-sns-control.md docs/implementation-plan.md`. If these files contain changes outside Phase 2 intent, compare Current state below with current code and STOP on disagreement.

## Status

- **Priority**: P1
- **Effort**: L (multiple days; includes core, stable schema, Candid boundaries, and regression tests).
- **Risk**: HIGH (first introduction of persistent state and public Candid boundaries underlying asset-moving operations).
- **Depends on**: None (assumes the Base contract ABI is frozen).
- **Category**: tech-debt / tests / direction
- **Planned at**: commit `5fc223c`, 2026-07-13

## Rationale

Base bSNS, Deposits, Withdrawals, pause, Timelock, and ABI snapshots were validated in Phase 1E, but the ICP Bridge canister still has an empty Candid service and the pure core has no domain logic. It cannot represent Deposit escrow, Withdrawal Release/Refund, EVM transactions, Reconciliation Hold, or post-upgrade resumption; no component can safely call the Base contract.

First build a deterministic core without external Ledger, EVM, or threshold ECDSA calls and canister state stored directly in IC stable memory. Separating external I/O into Plan 002 makes retries, idempotency, rollback, and terminal states subject to unit tests and Verus proofs.

## Current state

- `canister/bridge-core/src/lib.rs:1-4` — dependency-free Rust crate containing only a Phase 0 description; no types, states, transitions, or tests.
- `canister/bridge-canister/src/lib.rs:1-6` — exposes only `ic_cdk::export_candid!()`; no asset-moving or administration update methods.
- `canister/bridge-canister/bridge.did:1` — empty Candid service: `service : () -> {};`.
- `Cargo.toml:1-17` — workspace contains `bridge-core` and `bridge-canister`, pinning Rust 1.97.0, `candid 0.10.32`, and `ic-cdk 0.20.2`. No `ic-stable-structures` dependency yet.
- `scripts/ci-local.sh:53-63` — Rust gate runs fmt, clippy, workspace tests, Wasm build, and local-network preparation. Add core and schema tests here.
- `scripts/ci-local.sh:138-141` — ICP build gate runs only `icp project show` and `icp build bridge-canister`; domain Candid APIs and upgrade compatibility are not yet checked.
- `docs/adr/0008-handover-bridge-upgrades-to-sns-control.md` — requires direct stable-structure storage without whole-state `pre_upgrade` serialization and resumption of unfinished Deposits, Withdrawals, EVM transactions, and Reconciliation Holds after upgrade.
- `docs/implementation-plan.md:110-127` — Phase 2 defines state design, Deposit admission preserving Settlement Reserve, Service Fee protection, and Deposit flow. Pure logic precedes Phase 3 external integration.
- `docs/parameters.md:16-57` — Mint Throughput Limit, Per-Deposit Limit, `MAX_SERVICE_FEE`, and Settlement Reserve values are TBD. Plan 001 defines only raw-unit and checked-arithmetic contracts, without filling in values.

### Required terminology and constraints

- Use Deposit, Withdrawal, Bridge Exposure, Service Fee, Settlement Reserve, and Reconciliation Hold from `docs/glossary.md` in state names, comments, and test names. `Withdrawal Settlement` terminates through exactly one of Base `Pending → Released` or `Pending → Refunded`.
- Preserve ADRs 0001/0004/0005/0006/0008. In particular, refunds do not consume new Deposit mint throughput, Service Fees finalize only on confirmed success, and ambiguous Ledger transfers are not resent or refunded solely due to elapsed time.
- `docs/base-interface.md` and `contracts/abi/*.json` are authoritative for the Base ABI. Plan 001 does not change Solidity ABI.
- The core performs no external I/O. Introduce ICRC Ledger, EVM RPC, threshold ECDSA, timers, management canister calls, and HTTP only in Plan 002 or later.

## Commands you will need

| Purpose | Command | Success criterion |
|---|---|---|
| Drift check | `git diff --stat 5fc223c..HEAD -- Cargo.toml canister/bridge-core canister/bridge-canister scripts/ci-local.sh docs/adr/0008-handover-bridge-upgrades-to-sns-control.md docs/implementation-plan.md` | No unplanned Phase 2 changes |
| Rust format | `cargo fmt --manifest-path Cargo.toml --all --check` | exit 0 |
| Rust lint | `cargo clippy --manifest-path Cargo.toml --workspace --all-targets -- -D warnings` | Exit 0, no warnings |
| Unit/property tests | `cargo test --manifest-path Cargo.toml --workspace` | All core and canister tests pass |
| Wasm build | `cargo build --manifest-path Cargo.toml --target wasm32-unknown-unknown --release -p bridge-canister` | exit 0 |
| ICP build | `scripts/ci-local.sh icp` | Canister build, including Candid generation, passes |
| Full regression | `scripts/ci-local.sh checks` | Rust, contracts, SMT, Verus, and ICP build pass |

## Scope

**In scope (files allowed to change):**

- `Cargo.toml`, `Cargo.lock` — pin required dependencies such as stable structures.
- `canister/bridge-core/src/lib.rs`, `canister/bridge-core/tests/*.rs` — pure domain types, transitions, accounting, invariants, and unit/property tests.
- `canister/bridge-canister/src/lib.rs`, `canister/bridge-canister/bridge.did` — stable memory adapter and read-only Candid boundaries without asset movement.
- `canister/bridge-canister/tests/*.rs` — stable schema reopen/upgrade-equivalent tests. Any actual PocketIC upgrade tests also belong here.
- Phase 2 state-transition/stable-schema documentation under `docs/`, and proof-boundary additions to `verification/README.md`.
- Minimal CI test-invocation changes required by Plan 001; preserve the existing `contracts` gate and Base ABI.

**Out of scope (do not change):**

- `contracts/src/**`, `contracts/test/**`, `contracts/abi/**` — Base contracts and ABI frozen in Phase 1E.
- ICRC Ledger transfers, EVM RPC submission, threshold ECDSA, nonce queues, actual Settlement Reserve cost calculations, Runtime Administrator, and Fee Recipient operations — defer to Plans 002/003.
- TBD values in `docs/parameters.md`, Base Admin wallets, SNS Root handover, and mainnet/testnet deployment — defer to Plans 005/006. Production KINIC Ledger/Index IDs are already fixed. x402 facilitators are outside Bridge deployment/activation scope.
- Whole-state serialization in `pre_upgrade`. Stable structures' memory layout is authoritative.

## Steps

### Step 1: Fix state-transition tables and numeric boundaries first

Add Phase 2 transition tables under `docs/` for Deposits, Withdrawals, EVM transactions, and Reconciliation Holds. Specify states, allowed transitions, inputs, post-success storage effects, failure invariants, idempotent retries, and conflicting retries. Match Base `WithdrawalStatus` to Base records and use a separate enum for ICP execution state.

For Deposits, define relationships among `grossAmount`, user `maxServiceFee`, execution-time Service Fee, net mint amount, and Settlement Reserve reservations. For Withdrawals, define Base burn amount, `minAmountOut`, Ledger fee, Service Fee, Release/Refund outcomes, and retries. Choose `u128` narrowing versus Candid `Nat` only after documenting the target SNS Ledger maximum; no unjustified `as u128` or unchecked casts.

**Verify**: `rg -n "Pending|Released|Refunded|Reconciliation Hold|Service Fee|Settlement Reserve" docs/` finds all four state machines and numeric boundaries, with allowed and rejected transitions for each state.

### Step 2: Implement a dependency-free pure core

Add checked amount arithmetic, request identities, Deposit/Withdrawal/EVM/Reconciliation state types, `CoreError`, and deterministic transition functions to `bridge-core`. Transitions return new state or explicit errors without mutating input state on failure. Represent external calls as `Command` or side-effect-free decisions; the core does not access Ledger, EVM, or IC runtime.

Implement at least:

- Pre-Deposit checks for Service Fee cap, `maxServiceFee`, net mint amount, Per-Deposit/throughput inputs, rejecting admission when the Settlement Reserve reservation is insufficient.
- Idempotent, ID-bound decisions for Deposit pull, Base mint submission, confirmed success, failure, Reconciliation Hold, and refundable states.
- Mutually exclusive decisions for Withdrawal Base observation, Release submission/confirmation, Base Refund, and terminal states.
- No Service Fee credit before confirmed success; none on Base Refund or cancellation.
- Unknown Ledger outcomes remain in Reconciliation Hold; reject resubmission with another transfer identity and compensation without evidence.

**Verify**: `cargo test --manifest-path Cargo.toml --package bridge-core` passes pure core tests; `cargo clippy --manifest-path Cargo.toml --workspace --all-targets -- -D warnings` reports no warnings.

### Step 3: Add core invariant, idempotency, and boundary tests

Apply existing Solidity invariant concepts to the Rust core, building helpers that generate input states and command sequences. Test at least:

- Preservation of successful Deposit net amounts and fee reserves, unchanged state on failure, same-ID retries, and rejection of conflicting payloads.
- Exclusivity of Withdrawal `Pending → Released` and `Pending → Refunded`, successful identical terminal retries, rejection of different content, and no Refund after Release.
- Bridge Exposure preservation equivalent to `totalSupply + Pending + Released`, refunds not consuming Deposit throughput, and Service Fee changes not rewriting existing pending settlements.
- No direct transition from `ReconciliationHold` to a new transfer, refund, or Base compensation.
- Zero, maxima, fee greater than amount, duplicate IDs, empty payloads, unknown IDs, and arithmetic overflow/underflow.

If adding a property-testing dependency, verify Rust 1.97.0 support and test-only use. Otherwise prefer deterministic multi-case table tests with explicit coverage.

**Verify**: `cargo test --manifest-path Cargo.toml --package bridge-core` passes all cases above; `cargo test --manifest-path Cargo.toml --workspace` passes across all crates.

### Step 4: Implement the stable structures adapter and schema version

`bridge-canister` stores core state directly in stable SQLite instead of converting all state to a blob in `pre_upgrade`. Document record keys, stable value encoding, schema version, and migration policy.

The adapter calls core transitions and applies only successful decisions to stable maps. Phase 2 has no external I/O and exposes no asset-moving update endpoints. Limit queries to minimal nonsecret, nonsigning data such as state version, pause/admission state, unfinished counts, and Reconciliation Hold counts.

Close and reopen the same test memory, verify schema version, and confirm unfinished Deposit, Withdrawal, EVM transaction, and Reconciliation Hold records retain identical state. If reading an old schema is necessary, add explicit migration functions and fixtures; do not create missing assets through implicit defaults.

**Verify**: `cargo test --manifest-path Cargo.toml --package bridge-canister` passes stable-map write/reopen/schema checks; `cargo build --manifest-path Cargo.toml --target wasm32-unknown-unknown --release -p bridge-canister` exits 0.

### Step 5: Fix read-only Candid boundaries and regression gates

Make `bridge.did` match `ic_cdk::export_candid!()` output. Phase 2 Candid exposes read-only queries only; do not add updates allowing arbitrary callers to trigger Deposit/Withdrawal transitions. Align query record/variant names with Step 1 terminology; do not casually expose names reserved for future asset-moving APIs.

Connect only necessary Candid generation/schema tests to existing `ci-local.sh` Rust/ICP gates. Preserve Base contracts, ABI snapshots, SMT negative fixture checks, and Verus fixture checks.

**Verify**: `scripts/ci-local.sh rust` passes fmt, clippy, workspace tests, Wasm build, and local-network preparation; `scripts/ci-local.sh icp` passes project show/build; `scripts/ci-local.sh checks` passes all existing gates.

## Test plan

- Table-test all allowed/rejected transitions, terminal idempotency, and fee/reserve/exposure arithmetic in `canister/bridge-core/tests/`.
- Test stable-memory reopen, schema versions, old-fixture migration if adopted, and side-effect-free queries in `canister/bridge-canister/tests/`.
- Rust tests do not connect to real ICRC Ledgers or EVM RPC. External adapter tests belong to Plan 002.
- Use invariant, terminal-state, and idempotency patterns from `verification/smt/pass/WithdrawalState.sol`, `contracts/test/BridgeWithdrawal.t.sol`, and `contracts/test/BridgeInvariant.t.sol`; do not directly copy Solidity ABI or types.

## Historical done criteria

The unchecked entries below preserve the original plan checklist; they are not a current validation receipt. Current evidence is governed by the release claim ledger and current-source validation gates.

- [ ] `docs/` contains Phase 2 transition tables for Deposits, Withdrawals, EVM transactions, and Reconciliation Holds.
- [ ] `bridge-core` implements checked transitions, errors, idempotency, and fee/exposure/reserve invariants without external I/O.
- [ ] `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all --check` exit 0.
- [ ] Stable state is stored directly in `ic-stable-structures`; no whole-state `pre_upgrade` serialization exists.
- [ ] Stable schema reopen tests preserve unfinished Deposits, Withdrawals, EVM transactions, and Reconciliation Holds.
- [ ] Phase 2 Candid contains only read-only queries; arbitrary callers cannot trigger asset-moving transitions.
- [ ] `cargo build --target wasm32-unknown-unknown --release -p bridge-canister` and `scripts/ci-local.sh checks` pass.
- [ ] `git status --short` shows no changes outside Plan 001 scope.
- [ ] The 001 row in `plans/README.md` is updated.

## STOP conditions

- The target SNS Ledger amount maximum is unresolved, preventing a justified choice between `u128` and Candid `Nat`.
- Core transitions need external I/O, timers, caller identity, randomness, or current time.
- Unfinished state cannot reopen without schema changes, or the meaning of old fixtures cannot be recovered.
- Testing requires an asset-moving update method in the Phase 2 read-only boundary.
- Existing Base ABI, `contracts/`, or `contracts/abi/` must change.
- Clippy, workspace tests, Wasm build, or ICP build still fails after two reasonable repairs.
- A dependency addition cannot preserve Rust 1.97.0, pinned lockfiles, or wasm32 builds.

## Maintenance notes

- Plan 002 ICRC/EVM adapters should only call the fixed core transitions and stable keys/schema, translating external failures into states. Do not weaken core APIs for adapter convenience.
- Plan 003 Runtime Administrator assumes Phase 2 queries safely expose unfinished counts, reserves, and Reconciliation Holds.
- Plan 004 Verus proofs must cover pure-core transitions and invariants from the same production functions. Do not add separate unverified `Nat`/`u128` conversions.
- Review especially the absence of caller authentication at query/update boundaries, rejection of conflicting retry payloads, and preservation of upgrade compatibility through stable schema changes.
- Do not fill TBD values in `docs/parameters.md` in this plan. Update derivation formulas and external assumptions together in Plan 005 after target SNS and monitoring are fixed.
