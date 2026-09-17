---
status: accepted
---

# Centralize settlement execution in a stable SQLite job queue

Assume EIP-712 Mint Authorization from [ADR 0023](0023-use-wallet-funded-eip712-mint-authorization.md) and the Finalized guarantees and user-executed Withdrawals from ADR 0018.

Use `settlement_jobs` as the source of truth for Settlement execution state. For Deposits and fee payouts, a one-shot timer claims due jobs from SQLite. Withdrawals do not run on timers: each explicit `continue_withdrawal` manually claims the job. Timer IDs and heap-resident in-flight sets carry no persistent meaning.

A claim obtains a per-record lease and monotonically increasing generation in a SQLite transaction, renewing the lease immediately before RPC, signing, or Ledger calls. Checkpoints after await accept only the same generation. An old callback returning after an expired lease has been reclaimed cannot overwrite stable state with its external result. Automatic, Public Manual, and Governance Recovery lanes have separate concurrency limits, and leases for the same record must not overlap.

Deposits and fee payouts create or update records and jobs in the same transaction. Withdrawal notification atomically saves only the `ReleasePending` record and fixed Ledger transfer identity; it does not automatically register a job. Explicit continuation creates or reclaims the job. For Deposits, persist Authorization signature dispatch, the originating Finalized snapshot, fixed deadline, digest, and Service Fee finalization consistently with job progress. Release expired reservations by locally scanning the deadline index; save refund expiry/mint evidence and the Ledger transfer identity through an explicit owner action. For Withdrawals and fee payouts, save the Ledger transfer identity before external calls and recover through Ledger idempotency or Duplicate responses. Only the current lease generation may persist stopped, deferred, or completed fenced outcomes.

Authorization generation after admission and `continue_withdrawal` share the same runner and fencing, but Withdrawals are never automatically dispatched. Any non-anonymous Principal may call `continue_withdrawal`; it advances at most one external Ledger transfer or history reconciliation step, subject to existing quotas and the cycles floor. After complete proof of absence and saving a new identity, defer that transfer to the next explicit call. Nonterminal outcomes and stale scheduled Withdrawal jobs move to `Stopped`. Only `request_deposit_refund` starts a Deposit refund; it cannot bypass a valid normal lease, and any non-anonymous Principal may claim it only within rate limits and after the strict deadline. The recipient, amount, and transfer identity come solely from the existing record. After Authorization issuance, require `isDepositProcessed` reconciliation bound to a Base Finalized timestamp and canonical block; prohibit early refunds.

There is no Base mint transaction lane. Governance Operator nonces, signed raw transactions, and generations belong to a Governance signing lane separate from `settlement_jobs`. Broadcasting, waiting for Finalized receipts, and confirmation notifications are the external relayer's responsibility. Canister timers do not rebroadcast, monitor receipts, or automatically replace transactions. Replacement signs the same payload and nonce only on an explicit Governance request.

Derive public scheduler health from job counts and dispatcher diagnostics. Degraded/faulted indicators inform operations; they are not a global gate deciding whether individual jobs may run.
