# KINIC–Base Bridge

A bridge that maintains 1:1 backing for KINIC tokens between ICP and Base.

## Current status

The [implementation plan index](plans/README.md) is the source of truth for progress.

| Scope | Status | Remaining work |
|---|---|---|
| Plans 001–004 | Complete | Retained as historical records |
| Plan 005 | In progress | Finalize initial operating values and fixed limits. The 10-run, seven-day measurements and pause/cancel drills belong to Gate C after unpause |
| Plan 006 | Implemented in the repository | Gate B with 13 artifacts, production activation, and mainnet evidence. SNS handover timing is a separate decision |
| Plan 007 | Local complete / External pending | Nonblocking wallet compatibility checks and additional failure scenarios |
| Production | Deployed paused on both Base and IC | No asset admission until Gate B with 13 artifacts and production activation are complete |

`bridge-core` handles deterministic transitions for Deposits, Withdrawals, Mint Authorizations, Reconciliation Holds, Settlement Reserves, and accounting.
`bridge-canister` persists state in a single SQLite database with stable schema v36 and record wire v30, connecting the owner-sequence Deposit API, status queries, ICRC Ledger, EVM RPC, threshold ECDSA, and operational administration APIs.
For ICP→Base, the Canister checks state, fees, and pause status against a Finalized Base snapshot, then signs an EIP-712 Mint Authorization expiring 15 minutes after issuance according to IC consensus time. If fewer than five minutes remain when the signature is installed, it stops without accruing a service fee; it does not create or submit a Base transaction. Any Base wallet can submit `mintDepositWithAuthorization` with at least five minutes remaining, paying the gas itself. Solidity separately enforces a deadline no more than 15 minutes ahead of the current Base time.
After expiry, a bounded local scan in deadline order uses the existing Base Finalized snapshot to release only mint reservations. There are no per-Deposit timers, individual Base reconciliation calls, or automatic refunds. When any non-anonymous Principal explicitly calls `request_deposit_refund`, expiry and `isDepositProcessed` are checked at the same canonical Finalized block. If unprocessed, the Ledger refund uses the original account, amount, and transfer identity fixed in the record; if processed, the exact `DepositMinted` event and canonical receipt are saved and the Deposit advances to `Minted`. RPC disagreement, a missing event, or a digest mismatch must not move funds.
There is no mint ETH reserve, gas estimation, nonce, raw transaction, rebroadcast, or replacement. For Base governance, the Canister threshold-signs Governance Operator transactions, and only the external `governance-relayer` CLI broadcasts them, waits for Finalized confirmation, and notifies the Canister. Replacement is never automatic: only an explicit Governance request can re-sign the same nonce, at most three times, with a fee bump of at least 12.5%.
The Base implementation includes the ERC-20 representing KINIC (`name = "KINIC"`, `symbol = "KINIC"`), EIP-3009, Deposits and Withdrawals, independent pauses, fixed limits, Service Fee changes within the cap, and role rotation. Operations that increase risk go through OpenZeppelin's 24-hour Timelock.

For Base→ICP Withdrawals, the user submits `createWithdrawal`, which atomically executes bSNS `transferFrom`, burns the tokens, and enters `Committed` with a fixed payout in the same transaction. The Canister verifies the receipt, event, Withdrawal state, and Bridge snapshot by quorum, all bound to the same canonical Finalized block hash, and stores the liability to a fixed IC Account and the transfer identity. After successful notification, the UI calls `continue_withdrawal` once using the browser identity. If incomplete, each explicit History action advances the Ledger transfer or reconciliation by at most one external step. There are no Canister timer retries for Withdrawals, Base refunds, or release acknowledgements. If the Finalized head or canonical hash does not converge by 2-of-3 quorum, processing fails closed without falling back to Safe.

The production Bridge is deployed paused on both Base and IC. Production assets must not be accepted until Gate B with 13 artifacts (including initial operating values), the Canister-driven production preflight, and scheduled/executed activation are complete. The 10-run, seven-day measurements, pause/cancel drills, and five core scenarios are Gate C operational evidence collected after unpause; they do not authorize Gate B, activation, or controller handover. Operators decide SNS handover timing separately, while Plan 007's additional wallet compatibility checks and five additional scenarios continue as nonblocking work.

Start with the [documentation index](docs/README.md). See [docs/base-interface.md](docs/base-interface.md) for the Base ABI, [docs/bridge-flow.md](docs/bridge-flow.md) for execution flows, [docs/implementation-plan.md](docs/implementation-plan.md) for implementation phases, [docs/glossary.md](docs/glossary.md) for terminology, and [docs/adr](docs/adr) for safety decisions. [ADR 0024](docs/adr/0024-validate-rpc-chain-binding-before-runtime.md) is the source of truth for the guarantees and boundaries of RPC provider chain binding and runtime quorum.

## KINIC mainnet canister

| Role | Canister ID |
|---|---|
| Ledger | `73mez-iiaaa-aaaaq-aaasq-cai` |
| Index | `7vojr-tyaaa-aaaaq-aaatq-cai` |

The Bridge canister targets only this Ledger and Index. Ledger metadata is `name = "KINIC"`, `symbol = "KINIC"`, and `decimals = 8`. Archive canisters may be added, so their IDs are not fixed; use the Ledger's ICRC-3 archive discovery results.

The standard `bridge-canister` artifact requires Base mainnet (chain ID `8453`) and the Ledger/Index above at initialization. Arbitrary bindings for PocketIC and Anvil are accepted only by the `test-deployment` feature, which is disabled by default and built separately under `target/test-deployment/`. Never enable this feature for a production artifact.

## Pinned tools

| Tool | Version |
|---|---:|
| Rust | 1.97.0 |
| ICP CLI | 1.0.2 |
| ICP Rust recipe | `@dfinity/rust@v3.3.0` |
| ICP local network launcher | `v15.0.0-2026-07-02-07-40` |
| Foundry / Anvil | 1.7.1 |
| Solidity | 0.8.36 |
| OpenZeppelin Contracts | 5.6.1 (`5fd1781b1454fd1ef8e722282f86f9293cacf256`) |
| Z3 | 5.0.0 |
| Verus | 0.2026.07.05.49b8806 |
| Lean | 4.30.0 |
| Node.js | 24.14.0 |
| pnpm | 11.0.8 |

Rust is pinned in `rust-toolchain.toml`, Rust dependencies in `Cargo.lock`, Lean in `lean-toolchain`, the Solidity compiler and EVM target in `contracts/foundry.toml`, and OpenZeppelin by its Git submodule commit.
Rust 1.96.0, required internally by Verus, is pinned separately in the CI Verus installation step.

## Prepare a fresh clone

```bash
git submodule update --init --recursive
pnpm install --frozen-lockfile
pnpm --dir ui install --frozen-lockfile
pnpm --dir ui exec playwright install chromium
```

Install the pinned tools above, then run `scripts/ci-local.sh versions`.
See [`.github/workflows/ci.yml`](.github/workflows/ci.yml) for the CI installation procedure for pinned tools.

## Validation

During development, run the fast mode for the area being changed.

```bash
scripts/ci-local.sh rust-fast
scripts/ci-local.sh contracts-fast
scripts/ci-local.sh ui-fast
```

Run Wasm/PocketIC integration, coverage, and browser E2E checks individually as needed.

```bash
scripts/ci-local.sh rust-integration
scripts/ci-local.sh contracts-coverage
scripts/ci-local.sh ui-e2e
```

Solidity coverage requires LCOV lines ≥76.00%, branches ≥74.00%, and functions ≥68.00%. Missing LCOV, empty input, zero denominators, and invalid values fail the check; Timelock exclusions remain in place. Statement coverage is neither evaluated nor displayed.

For safety-related changes, distinguish these four completion points:

1. Implementation complete: code, tests, manifests, and documentation are updated.
2. PR validation complete: the proof stages selected from changed paths and the applicable tests pass. This does not generate a formal proof receipt.
3. Release validation complete: a complete proof receipt covers all 10 stages and matches the current source fingerprint.
4. Deployment approved: external changes such as a Canister upgrade, frontend publication, or Base transaction have explicit approval.

Passing proofs does not trigger deployment. Complete lightweight checks, including registration drift checks, before expensive proofs.

```bash
git diff --check
cargo fmt --all -- --check
python3 scripts/check_schema_consistency.py
pnpm --dir ui run codegen:abi:check
pnpm --dir ui run codegen:candid:check
python3 scripts/check_proof_impact.py
python3 scripts/check_claim_manifest.py
python3 scripts/check_claim_test_manifest.py --validate-only
```

For PRs, use the classifier's changed-paths JSON to run only impacted stages.

```bash
scripts/ci-local.sh proofs-impacted /path/to/changed-paths.json
```

This mode runs only the stage union selected by `verification/proof-impact.tsv` and does not create a formal proof receipt. The manifest selects all 10 stages for changes to production kernels, state transitions, Solidity policy, or proof infrastructure. Positive Lean stages always run together with their negative fixture stages.

After lightweight checks and applicable tests pass, confirm that there are no duplicate gates or writers, then run the complete proof gate once against the current fingerprint for the release candidate or production driver. Preserve the complete log for long runs.

```bash
qrun -- scripts/ci-local.sh proofs
```

PRs use the atomic gate matrix in `trusted-pr-gate`. Use the following only when full local validation is needed.

```bash
scripts/ci-local.sh checks
```

Main pushes and manual release candidates run full validation and the local deployment smoke test.

```bash
scripts/ci-local.sh all
```

Existing aggregate modes and other individual commands:

```bash
scripts/ci-local.sh versions
scripts/ci-local.sh rust
scripts/ci-local.sh contracts
scripts/ci-local.sh proofs
scripts/ci-local.sh ui
scripts/ci-local.sh icp
scripts/ci-local.sh smoke
scripts/ci-local.sh real
```

GitHub Actions `trusted-pr-gate` runs only the base branch classifier under `pull_request_target`, validating the PR's exact head SHA on a read-only, secret-free ephemeral runner. The classifier selects the atomic gates `policy`, `rust-fast`, `rust-integration`, `contracts-fast`, `proofs-impacted`, `ui-fast`, `ui-e2e`, `real`, `icp`, and `certora`. Documentation-only changes select no computation gates; deployment documentation requires only exact-head review. Workflows, core CI files, unregistered production sources, submodules, and unknown paths fail closed to all gates. Test, validation, and dependency changes require exact-head review in addition to applicable gates. PRs do not run `contracts-coverage`.
After the bootstrap merge, configure `trusted-pr-gate` as required and strict in Branch Protection or a Ruleset. Pushes to `main` do not use the classifier: the reusable `bridge-full-ci` runs `scripts/ci-local.sh all` once. There is no scheduled cron; use `workflow_dispatch` with a target SHA for failure investigation and release candidates.

`contracts` validates Phase 1A interface selectors and type ordering, concrete ABI snapshots, bSNS, EIP-3009, Deposits, Withdrawals, administration permissions, Timelock, stateful invariants, and LCOV coverage thresholds.
`proofs` builds Lean as the formal abstract specification of the cross-chain protocol and rejects `sorry` and `admit`.
Tracked conformance vectors generated from Lean are applied to the Rust, Solidity, and TypeScript implementations. Each vector section rejects specification, theorem, or consumer drift not registered in the manifest.
Manifest-registered consumers are checked for structural binding to their section and production symbol, then executed individually with an approved runner. A consumer counts as covered only when exactly one target test passes.
This comparison establishes limited conformance for enumerated boundary values, not complete semantic refinement of each language implementation.
The Deposit, Withdrawal, and administration decision cores shared with production are also proved with SMTChecker and Verus; deliberately underconstrained fixtures must be rejected. For non-executable Verus proofs, the registered specification is bound to `ensures`; for executable proofs, the registered kernel's return expression is bound to a named return and `ensures`. The obligation-side supporting claim set, claim-side reference set, and required production-bound evidence set must each match exactly. A claim cannot be promoted to proved production implementation unless every referenced Verus obligation is directly production-bound or covered as `derived` using only production-bound `executable`/`shared-expression` obligations for that same claim. Solidity proof links resolve full contract and overload signatures and the direct call graph in the compiler AST. For the Bridge wrapper, they bind the origin of `digest`, signature recovery, every `evaluateMint` input field, the origin of `effects`, absence of reassignment, and declaration IDs and ordering of commit arguments. Halmos symbolically checks state effects, mint amounts, and rollback on external call failure at the `_commitAuthorizedMint` boundary after signature verification. SMT and Halmos are both `supporting`; like Verus `derived` obligations, they provide partial claim evidence and cannot independently promote a claim to proved production implementation. Production transaction tests cover concrete end-to-end behavior, including wrappers, authentication, events, and persistence.
`ui` runs ABI/Candid drift checks, typechecking, lint, unit tests, builds, and desktop/mobile Playwright. `real` runs Playwright integration tests with the real Ledger suite and Anvil; it is included in `all` but not the shorter `checks` mode.
Proof scope and external assumptions are documented in [verification/README.md](verification/README.md) and [verification/obligations.md](verification/obligations.md).

Update ABI snapshots explicitly with the commands below. Normal CI only detects differences and does not update them.

```bash
python3 scripts/abi_snapshot.py --update
python3 scripts/abi_snapshot.py --check
```

After changing the Lean specification, explicitly update conformance vectors. Normal CI only checks for differences from generated output.

```bash
python3 scripts/protocol_vectors.py --update
python3 scripts/protocol_vectors.py --check
```

The 43 release claims, abstract/finite-width/trace theorems, typed Verus/SMT/Halmos obligations, explicit implementation bases, production links, transaction tests, and external assumptions are managed in [verification/claims.tsv](verification/claims.tsv); the gate computes evidence status. Vector consumers are managed in [verification/refinement-manifest.tsv](verification/refinement-manifest.tsv). Immediately before irreversible production operations, the proof gate is followed by two builds of the Wasm and contract runtime from clean source, requiring exact hash matches with the release manifest.

## Local deployment

`smoke` automatically performs the following:

1. Only when starting a new network, temporarily set `gateway.port` to an available port if port 8000 is occupied.
2. Start the local PocketIC network bundled with ICP CLI.
3. Deploy `bridge-canister` and verify `Running`, schema version 36 in `get_bridge_status`, and zero for all counts.
4. Start Anvil with chain ID 31337.
5. Deploy OpenZeppelin `TimelockController` with a 24-hour delay, the Canister-derived Governance Operator as the sole proposer/executor/canceller, and self-administration.
6. Deploy `Bridge` with the Timelock address as Base Admin, then verify the runtime bytecode, cross-references, and metadata of the bSNS created by its constructor.
7. Mint a smoke-test Deposit through the Bridge Signer and verify atomic burn and the fixed `Committed` quote through the user's `createWithdrawal`. Also verify that no additional Base transaction or re-mint selector exists for Withdrawals.
8. Verify Service Fee changes and pause through the Canister-derived Governance Operator, rejection of direct unpause by an external EOA, rejection of Timelock execution before 24 hours, and unpause through Canister execution after the delay.
9. Verify post-burn Withdrawal balances and supply, the mint window, and Withdrawal sequence numbers.
10. Stop only processes started by this script and restore the temporary changes to `icp.yaml`.

An already-running network for this ICP project is reused without changing its configuration or stopping it. Changes made independently to `icp.yaml` during the run are not overwritten. If another EVM node occupies port 8545, the script stops rather than reusing it.

Manual checks:

```bash
scripts/prepare_local_network.py --project-root . --write
icp network start -d --project-root-override .
icp network status --json --project-root-override .
icp deploy -e local --project-root-override .
icp canister status bridge-canister -e local --json --project-root-override .
icp network stop --project-root-override .
```

A manual `prepare_local_network.py --write` permanently changes `icp.yaml`. Restore the original port after stopping the network if needed.

The initial production deployment installed v35/wire v30. Migration to the current v36, which adds confirmed activation evidence, is atomic and runs exactly once during post-upgrade; normal reopen, other old or unknown schemas, dual reads, and fallbacks fail closed. Current staging likewise accepts only the reviewed v35→v36 upgrade that preserves the same Canister, deployment instance, and Base contract binding, followed only by current-schema upgrades. v7 staging evidence remains read-only and must not be resumed or migrated to v8.
