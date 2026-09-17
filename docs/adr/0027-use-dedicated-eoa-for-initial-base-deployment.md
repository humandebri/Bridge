---
status: accepted
---

# ADR 0027: Use a dedicated EOA for initial Base deployment

## Decision

A fresh dedicated EOA performs the initial deployments in order: Timelock, then Bridge. Only the relayer handles the EOA through an encrypted Foundry keystore and separate password file. The release profile fixes only the EOA address, starting nonce, predicted CREATE addresses, and gas/fee caps. A single `BASE_RPC_URL` is the transaction transport; do not save it in profiles, bundles, the UI, or evidence.

Immediately before submission, recheck Base Mainnet chain ID, pending nonce, balance, and both CREATE addresses. On nonce drift, do not submit; return for profile reapproval. If the outcome is unknown, track the same transaction recorded in the checkpoint; never automatically redeploy or submit the next nonce. A reverted receipt stops processing immediately.

Set the Timelock proposer/executor to the Governance Operator, canceller to the separately derived Independent Canceller, and additional admin to the zero address. Derive the Bridge Runtime Administrator separately from the Governance Operator. Fix Bridge administration to the Timelock; grant the dedicated EOA no role, ownership, or admin authority in Timelock, Bridge, or bSNS. Gate A approves offline artifacts and constructor conditions first. Verify post-deployment runtime, constructor postconditions, roles, pause, and cross-references using audit records obtained by the Bridge Canister from the official EVM RPC Canister's built-in `BaseMainnet`, binding them to Gate B. After deployment verification, recover remaining ETH, record the recovery transaction, and retire the keystore from operations.

## Consequences

The Canister holds no contract creation, deployment signing, deployment nonce, deployment replacement, or deployment confirmation state. Post-deployment Base administration uses separate threshold signers: Governance Operator for activation and control-plane rotation, Runtime Administrator for pause and Service Fee changes, and Independent Canceller for Timelock cancellation. Retrieving and broadcasting signed raw transactions are anonymously public; only confirmation that starts Finalized verification is restricted to the dedicated relayer Principal fixed in the release profile, Governance principal, or Pause principal. Normal signing requests and explicit replacements require Governance/Pause principals. A temporary exception permits the production controller fixed at seal time to perform initial schedule/execute and idempotent resume or bounded replacement of those pending transactions. Do not trust confirmation callers' reports; the Canister compares saved hashes with official EVM RPC Canister Finalized observations.

Authorize confirmation callers before singleflight and RPC, then apply inexpensive operation/hash checks and existing singleflight. This decision adds no stable rate limit or cooldown. The dedicated relayer identity receives no controller, Governance/Pause, EVM key, contract role, or asset-transfer authority.

Gate A represents Canister installation with Bootstrap operating values and completed paused Base deployment. Before initial activation, seal initial operating values derived from live information exactly once and complete the 13-artifact pre-seal/live Gate B. RPC rehearsals, monitor drills, at least seven days of Base fee measurements and at least 10 production governance gas/settlement cycles samples each, keeper drills, and monitoring receipts belong to Gate C after unpause. They are not controller handover authorization inputs, and measurements do not automatically update operating values. Controller handover and SNS upgrades are independent of initial activation and Gate C; perform them and connect them to the evidence chain only when operators separately approve the timing.
