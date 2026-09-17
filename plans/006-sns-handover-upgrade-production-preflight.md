# Plan 006: SNS handover, Canister-operated Base administration, and production preflight

## Status

- **State**: IN PROGRESS
- **Dependency**: Plan 005 initial operating values, fixed limits, and actual pause principal are finalized. Pause/cancel drills and seven-day, 10-per-type production measurements run in Gate C after unpause and do not authorize initial activation or controller handover.
- **Safety**: initial activation is complete and production assets are being accepted. Reactivation demonstrations and handover require approval separate from initial Gate B. Do not execute external transactions, controller changes, proposal submissions, or activation without individual approval.

## DAO demonstration while retaining the individual controller

Production is public and under test operation. Do not repeat initial activation; retain the individual sole controller while verifying DAO reactivation. Completed implementation and local validation do not authorize production proposals, pause/resume, or controller changes.

- The dedicated SNS validator and execution entry point accept the same typed payload: the most recently confirmed activation's Governance operation ID. The validator is read-only; execution authorizes only the Governance caller and delegates to the existing administration kernel.
- The production demonstration includes individual emergency pause, DAO schedule, execute through a separate proposal after 24 hours, relay, Canister Finalized confirmation, and small deposits/withdrawals. Proposal `executed` status alone is not completion.
- Reject stale payloads if the last confirmed operation changes. Resume the existing transaction for the same unconfirmed operation without creating duplicates.
- After the production demonstration, operators separately approve timing without a fixed waiting period, then perform the Root-only change, SNS registration, and same-Wasm upgrade in order.
- Production SNS registration removes the individual controller, so do not perform it during the period of retained individual authority.

See `docs/runbooks/dao-reactivation.md` for preparation and verification steps.

## Authority model

KINIC SNS Governance `74ncn-fqaaa-aaaaq-aaasa-cai` is the administration trust root for IC and Base. After final handover, the only long-term human-held administration credential is one IC emergency pause principal. There is no finance principal, release approver, or human Base Admin/Runtime/Canceller.

The Bridge Canister derives Mint Signer and Governance Operator on separate paths. Mint Signer only signs EIP-712 Deposit Mint Authorizations; it submits no Base transactions and holds no ETH. Governance Operator only signs Base pause, Service Fee, and Timelock schedule/cancel/execute; an external relayer submits and confirms them. Nonces and transaction records are not shared with the mint signing lane. Base administration APIs accept only closed enums, never arbitrary targets, calldata, raw transactions, or nonces.

## Fixed stages

1. From a clean revision, complete CI, Verus, ABI/Candid checks, current-schema reopen, and fail-closed rejection of unknown schemas.
2. Complete pre-activation safety evidence through production-like state upgrades on a same-Wasm test canister, PocketIC, and proofs. Move 10-run measurements, five launch-ready RPC scenarios, and pause/cancel drills to Gate C after unpause.
3. Install the initial Wasm on the production Canister paused, then fix the final pre-deployment profile plus five Bridge/BSNS build artifacts as a six-artifact offline Gate A bundle.
4. A temporary deployer installs Timelock and Bridge paused. Constructors assign roles only to the derived Mint Signer, Governance Operator, and Timelock, retaining no deployer role.
5. Finalize the post-deployment profile and Gate A receipt including deployment transactions/blocks. Preserve the deployed Gate A profile/receipt and initial install Wasm as immutable roots; never regenerate or reinstall them.
6. Normally upgrade from the production controller to controller-bootstrap Wasm. Preserve reproducible Wasm equality from clean source, a schema 1 upgrade receipt continuous from Gate A, pause-principal migration or current-template fresh-install no-op, and a schema 3 post-Gate-A policy transition. Preserve existing Candid method/argument ABI; add no public API beyond the two approved `ActivationConfirmationView` fields.
7. Production preflight on this workstation verifies canonical receipts, current module hash, source/upgrade chain, runtime hashes, role sets, zero deployer roles, and pause state.
8. After pre-seal Gate B with 13 artifacts including the current upgrade chain and policy transition, the production controller seals initial operating values exactly once.
9. After fresh live Gate B, the production controller prepares `schedule_activation`; anonymous relay and fixed confirmation-relayer confirmation schedule the 24-hour Timelock operation.
10. After 24 hours, create another fresh live Gate B. The production controller prepares `execute_activation`, executing only the recorded operation with the same role separation.
11. Resume IC Deposits automatically only after canonical Finalized success for both Base flows, permanently consuming internal bootstrap activation authority. Keep paused on failure, ambiguity, or drift.
12. Treat post-unpause seven-day, 10-per-type production measurements/Gate C independently from controller handover. Never hand over automatically; only at a separately approved time, change controllers to SNS Root alone and demonstrate an SNS proposal upgrade.

## Evidence contract

Keep Gate A immutable as deployed artifacts. Pre-seal Gate B verifies initial operating values and structural evidence; live Gate B verifies configuration from signature-verified queries, attestation, sole production controller, module/pause/reserve/cycles/pending state. Gate C adds post-unpause production measurements, monitoring, keeper, and upgrade history. There is no key ceremony or release approval. Verify Mint Signer by three-way agreement among profile, Canister public configuration from signature-verified queries, and fresh Finalized Base attestation. x402 is not a Bridge deployment/activation condition.

`monitor-drill.json` includes the pause principal, actual request ID, audit sequence, and audit digest. Fresh Gate B attestation is not initial activation approval itself; save controller activation authorization, preparation, confirmation, and Finalized status in separate receipts. SNS proposal IDs and execution evidence are used only for reactivation after handover; do not pass them with the Gate B hash to the Canister as self-reported values. Manifests are valid for at most 90 days; attach phase-specific authorization, artifacts, and receipts for schedule/execute to the same Gate B bundle.

## Completion criteria

- No permanent human EVM roles exist.
- Initial schedule/execute controller activation receipts are complete and internal bootstrap authority is consumed.
- If Gate C runs after unpause, the five core scenarios—`preflight`, `authorization_mint`, `withdrawal_release`, `quorum_loss`, and `final_pause`—reach `LAUNCH_READY` with raw artifacts. This is not a completion condition for initial activation or controller handover.
- Canister-originated Timelock schedule/execute and canonical Finalized receipts exist.
- Base and IC are active, with no controller, code, role, or reserve drift.
- Only if handover is separately performed, SNS Root-only control and SNS proposal upgrade have succeeded.

EIP-3009 is an optional bSNS integration feature; external facilitator compatibility does not block production readiness. Do not admit production assets before explicit initial-execute approval; handover approval remains separate.
