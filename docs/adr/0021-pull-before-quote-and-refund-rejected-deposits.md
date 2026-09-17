---
status: superseded
---

# Separate Deposit admission and Ledger pull with a stable executor

This ADR was superseded by the current funding-attempt approach, which removes admission DoS through unfunded intents.

Passing preflight does not guarantee a quote or mint reserve. The Canister saves a `FundingPending` record, stable executor job, fixed transfer identity, sequence, and quota in one transaction using the existing schema, then returns immediately. Only an executor holding the job lease performs the Ledger pull. Only confirmed success or Duplicate promotes the record to `EscrowedUnquoted`, after which pause, Service Fee, Per-Deposit Limit, Mint Throughput Limit, and reserves are revalidated against a fresh Finalized Base snapshot, current counters, and reserve token.

Definitive Ledger failures transition to existing `Cancelled`. An unknown result or lost callback saves `FundingReconciliationHold` and the transfer identity in the same transaction. Do not resubmit, cancel, or compensate without success evidence or a complete absence certificate covering tip, watermark, and contiguous segments.

Finalize the quote and mint reservation in a single storage transaction only after pull confirmation. RPC failures, provider disagreement, Bridge Signer mismatch, and stale observations do not justify refunds; stop in `EscrowedUnquoted` and observe again.

## Considered Options

- Reject performing the Ledger pull within the update call because a lost callback would lose the atomic source of truth for the formal record and transfer identity.
- Reject a funding-attempt table separate from formal records because it changes the schema and public history semantics.
- Reject resubmitting an ambiguous pull based solely on elapsed time because it could cause a double pull.

## Consequences

- Include `FundingPending` in formal Deposit counters, history, sequences, and jobs, but give it no quote, nonce, or mint reserve.
- Preserve public `DepositError`, schema v22, wire v18, and `FundingPending` history semantics.
- Lease callbacks update state only if the job ID, generation, and transfer identity satisfy CAS.
- Timer, manual, and confirmation paths cannot bypass Hold evidence requirements or lease claims.
- If races or state changes after preflight cause final admission to fail, use the existing refund path; users may pay Ledger fees for both pull and refund.
