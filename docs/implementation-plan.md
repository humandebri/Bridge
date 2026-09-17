# KINIC–Base Bridge implementation plan

> The former polling intervals and priority scheduler below are historical designs. The current sources of truth are ADR 0019's stable settlement executor, ADR 0023's wallet-submitted Mint Authorizations, and rate-limited manual Retry only after failures.

This plan follows the ADRs in `docs/adr/` and terminology in `docs/glossary.md`.
Use the definitions in `docs/glossary.md`; this document does not redefine them.

SNS Governance is the ultimate trust root for both IC and Base. The Bridge Canister signs Base operations through a Governance Operator derived on a separate path; an external relayer submits them. There are no human EVM administration keys.
Deploy the Bridge exclusively for KINIC (ADR 0010). Do not introduce branching for multiple SNS tokens.
Fix the mainnet Ledger to `73mez-iiaaa-aaaaq-aaasq-cai` and Index to `7vojr-tyaaa-aaaaq-aaatq-cai`. Discover archive canisters dynamically through the Ledger.

## Current progress

Base contract Phase 1E and Plans 001–004 are complete.
The Bridge canister implements stable schema v36, external integrations, Settlement Reserve, stable settlement executor, EIP-712 Mint Authorization, operational administration, and Verus proofs.
Plan 005's seven-day, 10-per-type production measurements and Plan 006's RPC rehearsal/monitor drill moved to Gate C after unpause. They do not authorize Gate B or controller handover. Retain SNS-proposal activation receipts for reactivation after handover. Initial activation separates seal/schedule/execute by the production controller fixed at seal time, anonymous relay, and the fixed confirmation relayer; Confirmed execute permanently consumes internal bootstrap authority. Operators separately decide when to remove the external controller; this is not automated. Even before removal, only the existing Governance principal has activation authority after initial execute. Plan 007 local staging and PocketIC/Anvil/frontend E2E are implemented; external runs for additional wallet compatibility and five additional scenarios await explicit approval but do not block production activation.

## Architecture

Implementation covers three components:

- **Base contracts**: bSNS ERC-20 and Bridge contracts. Non-upgradeable after deployment (ADR 0001).
- **Bridge canister**: the Rust canister on ICP, handling escrow, Deposit/Withdrawal state machines, and signing/submission to EVM. Upgradeable, with handover to SNS control at a separately approved time (ADR 0008).
- **Formal verification**: Verus for the canister and Solidity SMTChecker for contracts, covering obligations specified by each ADR.

Dependencies are as follows.
Finalize the contracts first because their interfaces—events, Withdrawal states, and fee constraints—are assumptions of the canister state machine.
Develop formal verification alongside each component, rather than adding it afterward.

## Phase 0: Foundations

- Define repository structure, separating `contracts/`, `canister/`, and `docs/`.
- Set up the Solidity toolchain: Foundry and SMTChecker.
- Set up the Rust toolchain: ic-cdk, ic-stable-structures, and Verus.
- Prepare a local runtime: PocketIC and anvil or an equivalent local EVM node.
- Establish CI for builds, tests, SMTChecker, and Verus.

**Completion:** empty contract and canister implementations deploy locally and CI passes.

## Phase 1: Base contract

Implement bSNS ERC-20 and the Bridge contract.
Because they are non-upgradeable, design errors in this phase require redeployment.
Implement every contract-side constraint imposed by the ADRs in this phase.

`docs/base-interface.md` is authoritative for constructors, types, functions, events, errors, and authority tables finalized in Phase 1A.
The Bridge creates bSNS in its constructor (ADR 0014); Base Service Fee is the source of truth read by the canister at Finalized blocks (ADR 0013).
The additional EIP-3009 interface is reflected in the Phase 1A specification and selector/topic tests (ADR 0015).
Phase 1B implements bSNS, EIP-3009, Deposit minting, Per-Deposit Limit, and fixed-window Mint Throughput Limit starting at deployment.
Phase 1C Withdrawals have been replaced by the current design: `createWithdrawal` atomically records transfer, burn, and a fixed quote as `Committed`. There is no Base acknowledgement, cancellation, or refund.
Phase 1D implements Service Fee changes, independent pauses, fixed limits, role rotation, and OpenZeppelin's 24-hour Timelock integration.
Phase 1E completes validation and freezes the ABI.

### 1-1. bSNS ERC-20

- Implement an ERC-20 backed 1:1 by Bridgeable SNS Tokens (ADR 0002).
- Provide no voting rights, neuron permissions, or Governance identity mapping (ADR 0002).
- Restrict mint and burn authority to the Bridge contract.

### 1-2. EIP-3009 authorized transfers (ADR 0015)

- Implement `transferWithAuthorization` and `receiveWithAuthorization` so x402 `exact` payments can settle bSNS directly.
- Implement `authorizationState` and `cancelAuthorization`, preventing reuse of used or cancelled nonces in a single namespace per authorizer.
- Bind the EIP-712 domain to the token name, fixed version `"1"`, execution chain ID, and bSNS contract address.
- Require caller/recipient equality in `receiveWithAuthorization`.
- Limit authorized transfers to existing balances; grant mint/burn authority to nobody except the Bridge.
- Use Foundry to test successful transfers, replay, validity boundaries, signer/domain mismatches, cancellation, and `receiveWithAuthorization` recipient checks.
- Preserve standard ERC-20 allowances and Permit2 as an alternative x402 payment path.

### 1-3. Deposit mint throughput control (ADRs 0001, 0012)

- Apply the Per-Deposit Limit to each Deposit.
- Apply the Mint Throughput Limit to total new Deposit mints in a fixed window (initially one hour). Include the factor-of-two boundary burst in limit derivation (`docs/parameters.md`).
- Make both limits and window duration immutable at deployment and define them in raw units. Do not use decimal display conversions for decisions.
- Provide no Base refund/re-mint path that reverses a burn by Withdrawal ID. Treat the Bridge Signer's normal Deposit mint authority as a separate trust assumption.
- Apply the Per-Deposit Limit to each Deposit and accumulate mints in the same fixed window against the shared Mint Throughput Limit.

### 1-4. Withdrawal state machine (ADR 0018)

- Base Withdrawals have only `None → Committed`, with `Committed` irreversible and terminal.
- Fix transfer, burn, Service Fee, `amountOut`, and IC Account in one `createWithdrawal` transaction.
- Create no post-ICP-transfer Base transaction or Withdrawal EVM operation.

### 1-5. Service Fee (ADR 0004)

- Fix immutable `MAX_SERVICE_FEE` in raw units at deployment.
- The contract also rejects fee changes outside `MIN_SERVICE_FEE <= service_fee <= MAX_SERVICE_FEE`.
- Compare Withdrawal `maxServiceFee` with execution-time Service Fee to protect users during fee changes.
- The Bridge pays Withdrawal Ledger Fees without reducing the user's fixed `amountOut`.

### 1-6. Administration authority separation (ADRs 0005, 0009)

- When monitoring detects balances insufficient for continued Withdrawal admission, the Runtime Administrator pauses new Withdrawals while existing Settlements continue. Neither the Bridge contract nor Canister pauses automatically.
- Assign immediate actions—pause and Service Fee changes within the cap—to the Runtime Administrator role.
- Only the Canister-derived Governance Operator executes delayed actions—unpause and role rotation—through Timelock. Use no human EVM administration key or hardware wallet; the delay is 24 hours.
- Expose no limit-changing functions or selectors.
- Grant Base Admin no authority over minting, refunds, or escrow assets.

### 1-7. SMTChecker proofs (ADR 0004)

- Service Fee cap constraints.
- Prevention of double fee accounting and fee finalization before success.
- Fee reserve preservation on recipient changes.
- Prohibition of payouts exceeding the fee reserve.

**Completion:** Foundry tests and SMTChecker pass, and Phase 1E ABI snapshots/fixtures freeze interfaces, including events and function signatures.

### 1-8. Contract validation and ABI freeze

- Track canonical concrete `Bridge` and `BSNS` ABI snapshots; check interface subsets and constructor, struct, and enum shapes in CI.
- Run Foundry fuzzing with 1000 runs and stateful invariants with 256 runs, depth 100, and `fail_on_revert`.
- Validate Deposit minting, Withdrawal exposure, terminal states, roles, and fee safety using production-shared libraries, unit tests, and stateful invariants.
- Test EIP-3009 authorization nonce namespaces and rollback through unit and fuzz tests.
- Require LCOV lines ≥76.00%, branches ≥74.00%, and functions ≥68.00%; reject empty, missing, or invalid LCOV. Do not use statement coverage.
- Record proof obligations and external assumptions in `verification/obligations.md`.

**Completion:** ABI snapshots, selector/topic fixtures, Foundry fuzz/invariants, SMT pass/negative checks, LCOV thresholds, local smoke, and CI agree and pass. ABI changes after Phase 1E require a separate plan and renewed review.

## Phase 2: Bridge canister state machine

First implement Deposit and Withdrawal state machines as pure logic with mocked external calls.
Separate ICRC ledger, EVM RPC, and threshold ECDSA calls to confine Verus proofs to deterministic logic.

Phase 2 implemented the deterministic state machine, initial stable schema, and observation queries.
Subsequent Plans 002/003 and current ADRs added external integrations, operational state, settlement executor, fund-before-formal-deposit, wallet-funded EIP-712 Mint Authorization, role-specific Governance nonce lanes, and confirmed activation evidence. The current format is stable schema v36.

### 2-1. State design (ADRs 0008, 0010)

- Dedicate the system to KINIC; remove token-ID branching from state and deployment configuration.
- Persist all state directly in ic-stable-structures; avoid serializing everything in `pre_upgrade`.
- Represent unfinished Deposits, Withdrawals, EVM transactions, and Reconciliation Holds so they can resume after upgrades.
- Before initial production deployment, replace stable schemas directly without migrations, dual reads, or fallbacks. Fail closed for any version other than current.
- Use only `bridge_metadata` as the source of truth for schema version; the current format is schema v36/record wire v30.
- Save Deposit record, owner sequence, and Base recipient in one envelope. Use corresponding index table counts as authoritative counts for pending EVM, open Holds, and nonterminal Withdrawals.
- Update Withdrawal primary rows, liability indexes, totals, and stop-reason aggregates together in typed SQLite transactions without relying on change-log triggers.

### 2-2. Deposit flow (ADRs 0001, 0004, 0005)

1. On admission, check local pause, inputs, and `gross_amount > 100_000`. Before paid Base preflight, save fixed transfer identity, consumed quota, and active reservation in a bounded funding attempt separate from formal Deposits, and check the cycle reserve.
2. After admission succeeds, persist `Dispatched` and perform the ICRC-2 pull. Only success or `Duplicate` permits paid Base preflight and promotion to a formal Deposit; a preflight failure does not discard funded assets. Definitive Ledger failure deletes the attempt and reservation and returns its quota within the same window. Ambiguous results or lost callbacks reconcile the same transfer identity.
3. Atomically finalize quote and mint reservation from fresh observations. Reobserve instead of refunding when observations are unavailable, inconsistent, or stale.
4. Before Authorization issuance, Base pause, fee rejection, or exceeded limits enter `RefundAvailable`; an explicit claim by any non-anonymous caller sends `gross_amount - 100_000` to the original account fixed in the record. After issuance, require the strict deadline and canonical unprocessed evidence, then send `gross_amount - charged_service_fee - 100_000`. Do not return the initial pull fee, finalized Service Fee, or refund Ledger fee. Ambiguous results enter Refund Reconciliation Hold and reconcile on renewed claims from any non-anonymous caller.
5. Credit the Service Fee to the fee reserve exactly once only when saving the Mint Authorization signature. Do not reverse it based on Base mint outcome; fee payouts use only finalized reserves.

### 2-3. Withdrawal flow (ADRs 0004, 0011, 0018)

1. Verify the `createWithdrawal` receipt, single event, `Committed` state, Bridge Signer, and runtime bound to the same 2-of-3 quorum Finalized block.
2. Save verification evidence, Withdrawal record, release job, transfer identity, and audit event in one SQLite transaction before starting the ICRC transfer.
3. Send fixed `amountOut = amount - chargedServiceFee`; the Bridge pays the Ledger Fee.
4. Transfer success or `Duplicate` terminates as `Paid`; unknown results reconcile complete history in Reconciliation Hold.

### 2-4. Separate accounting (ADRs 0004, 0005)

- Account for fee reserves separately from assets backing Bridge Exposure.
- Restrict fee payouts to finalized fee reserves so they cannot reach backing assets.
- A Fee Recipient change assigns the entire unpaid finalized fee reserve to the new recipient. Keep no per-recipient buckets.

**Completion:** unit tests verify every Deposit and Withdrawal state transition in a mock environment.

## Phase 3: External integrations

Phase 3 implements the ICRC adapter, Base Finalized monitoring, EIP-712 Mint Authorization, Governance-only threshold ECDSA signing lane, external Governance relayer, Reconciliation Hold history matching, and public Deposit API.
Use PicJS to verify preservation of Deposits, Withdrawals, and Holds through upgrades, and stuck receipts.

### 3-1. EVM integration (ADRs 0005, 0011)

- For Deposit mints, threshold ECDSA signs EIP-712 Authorizations and Base wallets submit transactions. There are no mint nonces, raw transactions, or gas reserves.
- An external EOA initially deploys Timelock, then Bridge, retaining no roles afterward. For Governance Operator, Runtime Administrator, and Independent Canceller lanes, the Canister retains independent nonces and signed generations; an unprivileged relayer broadcasts, waits for Finalized status, and notifies confirmation. There is no Canister rebroadcast, receipt timer, or automatic replacement. Re-sign only explicitly requested replacements, at most three times at the same nonce in the corresponding lane.
- Discover Withdrawal admission through `eth_getLogs` and confirm through Finalized-head state reads. Reads require agreement from two of three providers.

### 3-2. Settlement Reserve and stable executor (ADRs 0005, 0019)

- Maintain cycles floor and settlement cycle ceiling for signing, RPC, and Ledger processing. Replenish ETH separately up to the required cap for each Base-sending role: Governance Operator, Runtime Administrator, and Independent Canceller.
- Reserve logical mint capacity for unprocessed Authorizations until terminal state, but exclude mint gas and ETH reserves from Deposit admission.
- Stable executor jobs use typed-kind claim policies and per-record leases; do not conflate Deposits, Withdrawals, and fee payouts.
- Lease generations increase monotonically. Reject stale callbacks, duplicate claims for one record, and generic manual claims of in-progress scheduled jobs. Automatic, public manual, and Governance recovery lanes have independent caps.
- Stop new Deposit admission when cycles constraints or logical mint capacity cannot be met.
- Document and audit Governance gas price, EVM RPC cost, and management canister call cost upper bounds as external assumptions.

Settlement Reserve, stable executor, new Deposit pause, Fee Recipient, fee payouts, and stable audit logs are implemented according to Plan 003 and ADR 0019.
Plans 005 and 006 finalize production values and key custody.

### 3-3. Reconciliation Hold (ADR 0006)

- During deduplication, retry only with identical `created_at_time`, memo, amount, fee, from, to, and spender.
- Afterward, reconcile ICRC-3 and index history, checking complete search coverage including archives and synchronized watermarks; never match by memo alone.
- Do not conclude absence while history services lag, data is missing, or archives fail.
- Retain requests with unresolved outcomes in Reconciliation Hold indefinitely; prohibit resubmission, Deposit refunds, or Base Refunds based on elapsed time.
- Limit Governance resolution to evidence-based success/failure determination; the API must not permit forced resubmission or refunds without evidence.

**Completion:** PocketIC/local EVM integration tests verify success, failure, and Reconciliation Hold transitions.

## Phase 4: Administration authority

Plan 003 implements administration authority and audit logs.

- The single pause principal can only pause IC/Base, cancel recorded pending Timelock operations, and advance permitted Settlements.
- Only SNS Governance may perform normal resume, pause principal rotation, Fee Recipient changes, fee payouts, Service Fee changes, and Timelock schedule/execute. The production controller fixed at seal time is a temporary exception only for initial activation schedule/execute.
- Separate Mint Signer from Governance Operator, Runtime Administrator, and Independent Canceller administration lanes. Expose no arbitrary target, calldata, raw transaction, or nonce API.
- Grant no permanent roles to human EVM addresses, controller identities, or the initial deployer.
- Do not automatically convert SNS-token fees into Base gas ETH; operators replenish according to the runbook.

## Phase 5: Formal verification (Verus)

Plan 004 implements production-shared kernel proofs and negative fixtures.
Rerun proofs for each Wasm; do not reuse a previous version's proofs for a new upgrade (ADR 0008).

- Each Deposit quote satisfies the Service Fee cap, positive net amount, Per-Deposit Limit, and Mint Throughput Limit; mint reservation occurs only at quote finalization. There is no Withdrawal-specific Base refund/re-mint path, and processed Deposit IDs cannot replay (ADRs 0018, 0021).
- Assuming canonical observations and Ledger success, a Withdrawal progresses from Base `Committed` to Canister `Paid`, after which it cannot be resent, reduced, or redirected (ADR 0018). Do not claim external service liveness.
- Service Fee caps, no double accounting, no fee finalization before success, reserve preservation on recipient changes without unfinished payouts, and no payouts exceeding the fee reserve (ADR 0004).
- Deposit admission meets the Settlement Reserve including the candidate, and transition from candidate to reserved does not reduce required resources (ADR 0005).
- Stable executor leases bind to records and lanes, generations increase monotonically, and stale callbacks, duplicate claims, and manual bypass of scheduled/leased jobs are rejected (ADRs 0019, 0023).
- Separate release claims into `Claims.lean`, finite-width models into `FiniteWidthModel.lean`, model refinement into `ModelRefinement.lean`, and integrated traces into `Protocol.lean`. CI requires exact agreement among claim ledgers, vector sections, production consumers, and external assumptions. Treat model refinement as bounded conformance through generated vectors, not proof of the entire production implementation. Release drivers reject self-reported attestations and rerun the proof gate and double artifact build from clean source immediately before irreversible operations.
- No direct transition from Reconciliation Hold to a new transfer or compensating state (ADR 0006).

Proof scope is limited to 1:1 asset backing and these properties; it excludes cross-chain governance (ADR 0002).

## Phase 6: Initial activation, production measurements, and SNS handover

- Retain the production controller as sole controller, seal fixed operating values, and perform initial schedule/execute only after Gate B and separate approval.
- Confirmed completion of initial execute permanently consumes internal bootstrap activation authority; subsequent operations authorize only the Governance principal even without external controller changes.
- Using production-like state, verify that unfinished Deposit Authorizations, Withdrawals, Governance EVM transactions, and Reconciliation Holds resume across upgrades.
- After unpause, collect at least seven days and 10-per-type production measurements and Gate C evidence. Results neither automatically update operating values nor authorize or automatically schedule handover.
- If handover occurs at a separately approved time, bind initial operating values, seal/schedule/execute receipts, live RuntimeBinding, and the post-Gate-A upgrade chain to the current profile Wasm. Immediately before submission, require the production identity as sole controller, Activated state, and unpaused Base flows and IC Deposits. Preserve pre/post-change module, runtime, storage integrity, and operational continuity. Do not require empty state; afterward SNS Root must be the sole controller, retaining no developer identity, fallback identity, or NNS Root.
- Automate CI generation of artifacts for post-handover upgrade proposals: Wasm hash, source revision, Verus results, test results, and stable schema compatibility.
- EIP-3009 is an optional bSNS integration feature; x402 resource server/facilitator compatibility is not a Bridge deployment or activation condition (ADR 0015).
- The UI must state before Deposit that bSNS provides no voting rights or voting rewards (ADR 0002). If implemented in another repository, hand over this requirement.

**Completion:** initial activation completes with a controller activation receipt and verified active state. SNS handover is assessed separately after operators approve its timing and the handover checklist and SNS proposal upgrade are completed.

## Remaining work

Plan 005's initial activation prerequisites are approved initial operating values, a single pause principal, fixed limits, and pre-seal/live Gate B. Collect RPC rehearsal, monitor drill, at least seven days of Base fees, and at least 10 production Governance gas/settlement cycles samples each in Gate C after unpause, without automatically changing configuration. Treat 5/15/60 as post-publication monitoring targets, not production gates.
Do not mark the initial activation mainnet candidate `validated` until authenticated Gate A/Gate B and schedule/execute activation receipts are complete.
Plan 006 repository implementation is complete. Actual SNS Root controller handover and SNS proposal upgrade remain separate completion conditions after operators independently approve timing.

## Phase dependencies and milestones

| Phase | Scope | Prerequisites |
|---|---|---|
| 0 | Foundations | None |
| 1 | Base contract | Phase 0 |
| 2 | Canister state machine | Phase 1 interface freeze |
| 3 | External integrations | Phase 2 |
| 4 | Administration authority | Phase 2; may run alongside Phase 3 |
| 5 | Verus proofs | Alongside Phase 2 onward |
| 6 | Initial activation, production measurements, SNS handover | All Phases 1–5; target SNS token, initial operating values, and key management details finalized |
