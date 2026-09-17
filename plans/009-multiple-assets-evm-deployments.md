# Plan 009: Expand to multiple assets and EVM chains while preserving KINIC on Base

Status: design direction agreed; implementation not started.
Based on user instructions dated 2026-09-08.

## Scope and deployment unit

Bridge multiple ICP assets to multiple EVM chains.
A **deployment unit** is one Bridge Canister, Bridge contract, and issued token corresponding to one ICP Ledger and one EVM chain.
The **existing deployment** means the deployed KINIC pair on Base; **additional deployments** mean separate pairs created later.
Use a separate Canister even when adding the same asset to another chain.
The shared UI selects assets and destinations from verified deployments.

Exclude non-EVM chains, direct EVM-to-EVM transfers, and multi-asset custody in one Canister.
Specific additional assets and chains are not yet selected and must not be represented as supported.

## Preserving the existing deployment

[Bridge.sol](../contracts/src/Bridge.sol) creates its token in the constructor and holds `bsns` immutably.
[BSNS.sol](../contracts/src/BSNS.sol) immutably assigns mint/burn authority to its creating Bridge.
Therefore, this plan excludes moving the existing token to a new Bridge.
Preserve existing addresses, ABI, events, EIP-712 domain/types, signature verification, and eight-decimal token representation.
Do not require fund movement, token swaps, reinstall, or reactivation of the existing deployment for expansion.

Before implementation, identify the existing deployment artifacts and deployment source revision, fixing runtime hashes, ABI, and known signature vectors.
Do not assume current source matches deployed bytecode.
During implementation, run existing-deployment connectivity tests against that fixed evidence.

Preserve the policy recorded at this plan's creation: production Canister schema v35 and current source at undeployed v36.
Do not weaken existing UI authorization constraints to the v35 evidence chain or the restriction that normal current-release Gate B accepts only v36.
Issue evidence separately for each additional deployment; never reuse existing deployment receipts.
Follow the production compatibility policy in [AGENTS.md](../AGENTS.md) for details.

## Additional deployment design

Separate custody accounts, accounting, unfinished operations, history, pause, limits, and cycles per Canister deployment.
Even with the same Ledger, separate backing through accounts owned by different Canisters.
Apply existing accounting invariants per deployment, including mint reservations, refund liabilities, Withdrawal liabilities, and fees.
Add no mechanism to cover one deployment's shortfall from another's balance.

At install, bind the Ledger and required Index, chain ID, Bridge/token addresses, runtime hashes, RPC set, signing-key derivation paths, token information, fee policy, and limits.
Provide no runtime destination-switching feature.
Preserve existing signing keys; use deployment-distinguishing derivation paths for additional deployments.
Preserve signature binding through chain ID and verifying contract; reject another deployment's Authorizations, receipts, or history.

Limit additional assets to Ledgers supporting the operations and history reconciliation currently used.
Do not infer approved-transfer, deduplication, archive-discovery, or Index compatibility merely from an ICRC label.
Initially require matching ICP/EVM decimals without unit scaling or rounding.
Replace fixed KINIC Ledger fees with explicit per-asset policies, but define reconciliation/retry behavior on fee changes before implementation.
Supported amounts must satisfy existing u128 constraints.

Adopt additional chains only after validating opcodes, signature schemes, receipt/event formats, Finalized semantics, RPC quorum, and Governance transaction fee calculations.
Do not reuse Base-specific L1 fee assumptions on other chains.
Preserve [ADR 0024](../docs/adr/0024-validate-rpc-chain-binding-before-runtime.md)'s fixed-provider assumptions and pre-deployment chain-binding checks.
Once destinations are selected, consult current primary documentation for each chain and RPC.

Additional-deployment contracts must allow token name, symbol, and decimals to be fixed at deployment.
Share code with the existing deployment only where actual ABI and signature specifications remain preserved.
Replace undeployed configuration formats together with callers and fixtures; add no old-format fallback.
Do not simply remove production's fixed-KINIC checks to permit arbitrary configuration; replace them with authorization conditions for verified deployments.

## Deployment selection in the UI

Resolve asset/chain selection to exactly one complete deployment profile.
Do not identify assets by symbol alone; verify Ledger, chain ID, Canister, contract, and deployment-instance bindings.
Do not accept arbitrary user-entered RPC endpoints or contracts as production deployments.

Isolate history, pending transactions, query caches, and asynchronous responses per deployment.
After selection changes, keep in-progress operations bound to their originating deployment.
Test that responses from an old view cannot trigger transfers or confirmations in the new deployment.
Request EVM transactions only when the wallet's connected chain matches the selection.
Completion requires continued access to existing KINIC-on-Base history and recovery operations.

## Implementation order and validation

1. Fix regression baselines for existing artifacts, ABI, signature vectors, profile authorization, and history. If deployed evidence cannot be identified, do not begin changes affecting existing connectivity.
2. Select candidate Ledgers/EVM chains and finalize compatibility, fee policy, operators, and cycles costs at the planned deployment count.
3. Implement install-fixed configuration and authorization conditions; apply single-asset Canisters/contracts to additional deployments, validating local fixtures first.
4. Implement shared UI deployment selection and history isolation.
5. Locally validate the same asset on another chain and another asset on the same chain, confirming rejection of cross-deployment confusion.
6. Freeze source and obtain staging/release evidence per additional deployment. External deployment and existing-Canister upgrades are separate operations, each with concrete targets and evidence.

Before changing safety logic, enumerate affected claim IDs from [proof-impact.tsv](../verification/proof-impact.tsv) and [claims.tsv](../verification/claims.tsv).
Map each claim to its abstract theorem, production kernel, proof obligation, negative fixture, refinement/adapter test, transaction test, vector consumer, and external assumptions.
This plan changes no logic and does not update claim proof status.

During implementation, run manifest checks and impacted proof stages.
PR validation includes `python3 scripts/check_proof_impact.py`, `python3 scripts/check_claim_manifest.py`, `scripts/ci-local.sh proofs-impacted <changed-paths.json>`, and applicable tests.
Verify UI changes with Playwright CLI.
Release candidates run `scripts/ci-local.sh all`; production drivers run `scripts/ci-local.sh proofs` with a complete current-source receipt.
Follow existing AGENTS.md rules for lightweight prerequisites, one writer, frozen inputs, and receipt verification before expensive validation.

Acceptance tests include existing-deployment bidirectional flows/recovery, additional-deployment bidirectional flows, differing decimals, fee mismatches, rejection of another deployment's signatures, chain mismatches, receipt confusion, async responses during UI switching, and per-deployment pause.
New-chain finality/provider independence, external Ledger behavior, and key/Governance trust remain external assumptions; do not promote unproved claims to complete.

## Deliverables and work not performed

This change adds only this plan and its index entry.
Additional asset/destination selection, implementation, test execution, live chain queries, deployment, and Canister upgrades have not been performed.
Existing in-progress code changes in the working tree are not deliverables of this plan.
