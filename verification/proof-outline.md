# A 30-minute guide to the Bridge proofs

## Propositions and assurance targets (5 minutes)

A **claim contract** is the Lean proposition for a release claim.
**Production evidence** consists of Verus obligations for shared kernels, finite vector comparisons, and transaction tests supporting that proposition.
See the [claim ledger](claims.tsv) for their connections and external assumptions.
The [specification correspondence table](claim-semantics.tsv) explains premises and conclusions; the [major definitions table](definition-semantics.tsv) centralizes shared concepts.
These new tables do not compute evidence strength.

In the [statement catalog](generated/claim-statements.md), follow witness types to proposition definitions and check whether premises are too strong or conclusions omit part of the specification.
For example, fee accounting once at signing and the minimum remaining validity period are separate conditions; check that the proposition includes both.

## From accounting deltas to histories (10 minutes)

GlobalHistory backing requires escrow to equal the sum of Base supply, fee reserve, unminted liabilities, and unreleased liabilities.
Signing moves value from unminted liabilities to fee reserve; minting moves it from unminted liabilities to Base supply.
Refunds reduce escrow and unminted liabilities by the same amount.
For payouts, the sum of the escrow decrease and fee reserve increase equals the decrease in unreleased liabilities.

These deltas are applied to the record and aggregate totals together.
The single-step preservation theorem preserves ID uniqueness, agreement between record sums and aggregates, backing, and the requirement that records holding reservations are Deposits.
A separate lemma shows that records with other IDs remain unchanged.
Induction on the length of accepted histories extends single-step preservation to the entire history.

Here, `AccountingInvariant` does not include authentication conditions for actual Ledger operations.
`ModelBoundaries.accounting_invariant_does_not_certify_payment` gives an example where a record satisfies accounting conditions yet moves to paid through a callback without a transfer.
This fixes the accounting model's expressive boundary; it does not claim that production accepts that callback.

## Individual guarantees and abstractions (8 minutes)

DepositHistory covers histories from authorization issuance to termination.
Signature events receive an IC observation timestamp and check the deadline's u64 range, rejection of addition overflow, and at least 300 seconds of remaining validity.
Exactly 300 seconds is accepted; 299 seconds, expired deadlines, and addition at the maximum timestamp are rejected.
Correspondence with the production minimum-remaining-time predicate is linked through a finite-width model theorem and the Rust vector consumer.
Cryptographic signatures themselves and IC timestamp authenticity remain external assumptions.

The integrated Protocol proves that the stored quote's recipient and net amount remain unchanged across transitions.
Extending this preservation rule across histories shows that final withdrawal values match the initial state's stored quote.
`pending_payout_is_bounded_by_reserve_across_trace` relates payout reservations to reserves; local predicate theorems check Service Fee bounds.

`filterSafeStoredState` is an abstract filter accepting states under the Safe condition.
A proof that its output is Safe does not establish safety of actual SQLite decoding or schema migration.
Production restoration depends on existing Rust and PocketIC transaction tests, SQLite atomicity, and external fixed-schema conditions.

Conditional liveness derives reachability from continuous admissibility of the target terminal operation until selection, availability of required external operations, and weak fairness of the dispatcher.
No proof derives this admissibility from receipt of funds.
`DepositTerminalProgressLemmas` is the conjunction of mint and refund implications, not an either/or proposition about one shared execution.

## Reviewing changes (7 minutes)

First inspect premises, conclusions, and unproved boundaries in the specification correspondence table.
Then review statement and major-definition diffs for lost relationships to initial values, finite-width constraints, authorization conditions, or acceptance conditions.
Return to the claim ledger for implementation correspondence, distinguishing obligations that check shared kernels from model-only lemmas.

After changing a proposition or major definition, update its correspondence-table description or `review_note`.
Regenerate artifacts with the following commands.

```sh
python3 scripts/check_claim_semantics.py --write
python3 scripts/check_claim_semantics.py --base-sha <40-character-trusted-base-SHA>
```

The comparison baseline is read from the specified Git commit.
Rewriting the same candidate's JSON cannot eliminate review requirements.
Trusted CI uses the existing `BRIDGE_TRUSTED_BASE_SHA` and retains the exact-head review requirement for changes to `verification/`.
An initial introduction with no snapshot in the baseline is reported as bootstrap.
While the existing trusted base lacks the new checker code, local verification and the existing bootstrap procedure remain necessary.

Source digests also report comment or proof-body changes.
This is a conservative prompt for human diff review, not proof that semantics changed.
Changes to dependent definitions or structures outside the major-definition list also appear in source digests; review the corresponding source diffs.

Independent kernel checking has not been introduced.
Do not report passing axiom dependency checks and ordinary Lean builds as independent kernel results.
