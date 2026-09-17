---
status: accepted
---

# Keep ambiguous ledger transfers in Reconciliation Hold

> ADR 0021 supersedes the Deposit refund prohibition in this ADR for definitive post-pull rejection and dedicated refund reconciliation. This ADR still governs the prohibition on compensating ambiguous funding outcomes.

If an ICRC transfer's outcome remains unknown after the deduplication period and history cannot fully establish success or failure, keep the request in Reconciliation Hold indefinitely. Prohibit resubmission, Deposit refunds, and Base Refunds based solely on elapsed time.

After complete proof of absence, a Deposit terminates as non-reusable Cancelled. Only a Withdrawal may increment its attempt number and create a new Transfer Attempt, preserving the economic payload except for created-at time and memo.

## Considered Options

- Reject creating a new transfer or compensation after timeout because a previously successful original transfer would cause a double pull or double payment.
- Adopt waiting until success or failure can be proved, prioritizing asset safety despite possible loss of availability.

## Consequences

- Within the deduplication period, each explicit Continue resubmits once with the same `created_at_time`, memo, amount, fee, from, to, and spender. Never resubmit automatically.
- After that period, each explicit Continue performs only one step of at most four calls scanning ICRC-3 and index history, verifying search completeness including archives and a synchronized watermark.
- Do not match by memo alone; compare operation, from, to, spender, amount, fee, and created_at_time.
- Do not conclude absence while history services lag, data is missing, or archives are unavailable.
- Governance may finalize success or failure based on evidence but cannot force resubmission or refunds without evidence.
- Use Verus to prove that Reconciliation Hold cannot transition directly to a new transfer or compensating state.
- `next_block` is the first unscanned block and `ledger_tip` is inclusive. Absence requires `next_block > ledger_tip`, reaching the index watermark, complete archives, and no exact transfer match.
- Accept the residual risk that assets remain blocked forever if external history can never be verified.
