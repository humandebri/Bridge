# Plan 003: Settlement Reserve / Runtime Administrator

> **Historical record:** this document records implementation boundaries at Plan 003 completion. The current implementation uses explicit Settlement operations.
> See the repository-root `README.md` and `docs/` for current specifications.

## Status

- **Priority**: P1
- **Risk**: HIGH
- **Depends on**: Plan 001, Plan 002
- **State**: DONE

## Implemented boundary

- Reserve ETH and cycles conservatively in separate units; checked-compute required Settlement Reserve from nonterminal Withdrawal counts. If balances are unavailable or insufficient, reject only new Deposits before ICRC pull.
- Fix EVM operation calldata as Queued intents without nonces, assigning nonces to acknowledgement/refund before mint. From Prepared onward, preserve nonce order and identical raw transactions.
- The multiple-admin design from this period was replaced by Plan 006. Currently, one pause principal performs safety actions only; SNS Governance handles Fee Recipient, fee payouts, resume, and role management.
- Before fee payout submission, persist amount, Ledger fee, recipient, and transfer identity in stable memory. Debit fee reserves only on success or Duplicate; Hold ambiguous results until history reconciliation.
- Append-only audit logs retain pause/resume, rotation, Fee Recipient, fee payouts, reserve gates, and observed Base Service Fee changes in sequence order.
- Under the pre-production policy, accept only current schema v4 with no legacy migration.

## Verification

- Rust core checks reserve boundaries, overflow, fee-reserve arithmetic, and EVM state ordering.
- PicJS checks asset-moving sagas, pause/resume, no pull under reserve shortage, audit logs, and fee payouts on PocketIC.
- `scripts/ci-local.sh checks` runs Rust, Wasm, Candid, PicJS, ICP, Foundry, SMT, and Verus checks together.

## Deferred

- Plans 005/006 finalize production reserve values, key custody, and mainnet deployment.
- Introduce no fee bumps, manual Hold resolution, or arbitrary transaction submission.
