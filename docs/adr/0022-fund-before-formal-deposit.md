---
status: accepted
---

# Create formal Deposits only after successful Ledger funding

`request_deposit` creates a stable funding attempt in `Prepared`, separate from formal Deposits. One transaction consumes the low-cost admission quota, adds an active funding reservation, and checks the cycle reserve. Rejection by quota or cycle reserve must not start Base RPC or an ICRC-2 pull. After admission, perform the fixed-identity ICRC-2 pull first; only requests bound to assets by Success or Duplicate proceed to fresh Base preflight at the Canister's expense. Unfunded Sybil Principals cannot consume shared verification capacity or Base RPC cycles.

On Success or Duplicate, atomically promote the attempt to a formal `EscrowedUnquoted` Deposit regardless of the preflight result. After Ledger asset movement, failure without creating a public record is prohibited; resolve Base rejection or temporary failures through settlement retries or refunds. Unknown Ledger results promote to `FundingReconciliationHold` bound to the same identity. Definitive Ledger failures such as Insufficient Allowance or Insufficient Funds delete the attempt and active reservation, leaving no record, history, sequence, or job.

Retryable failures retain the same identity for only 120 seconds. Resubmission before the retry deadline does not repeat Base preflight; after the deadline, the saved attempt still prevents double quota consumption. Lost callbacks are reconciled against Ledger history by a dedicated low-priority recovery scan. Success promotes the attempt; release the reservation only when a fresh scan begun strictly after the 24-hour deduplication period plus a 60-second subnet-time safety margin proves complete absence.

Deposit pause rejects creation of new funding reservations. Even if pause begins after reservation creation, a successful or ambiguous irreversible Ledger pull must promote to a formal Deposit, preventing user funds from being stranded without a public record.

There is no public `FundingPending` state. Return `FundingRejected` for definitive failures and `FundingUnavailable` for temporary failures. At the time of this decision, the format was directly replaced with schema v35/wire v30. Following production deployment, allow only the one-time post-upgrade v35→v36 migration adding confirmed activation evidence; all other old or unknown versions fail closed.
