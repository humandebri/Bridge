# Deposit and Timelock audit response

## Change and evidence scope

Add refundability and recipient conditions to Canister signing eligibility. Existing unsigned requests may proceed to refund, but do not apply the new policy retroactively to requests with saved signatures. GlobalHistory distinguishes released reservations from completed refunds and adds accepted history from reservation release through refund completion.

Timelock constructor changes are for future deployments and have not been applied to live contracts. Do not replace approved runtime hashes or deployment evidence for existing production with new bytecode. Make separate release decisions for Canister/UI changes and Solidity changes. Preserve the distinction between production v35 and undeployed v36 and the UI's v35-only authorization recorded at this review.

## Existing production Deposits

On 2026-09-08 JST, an anonymous `get_bridge_status` query to `lb5i5-ziaaa-aaaar-qcgwq-cai` reported schema 35, total Deposits 0, retained deposit index entries 0, pending ledger operations 0, reconciliation holds 0, and reserved mint operations 0. There were no existing Deposits to classify at observation time. Public lists are owner-specific; this zero determination uses aggregate counts. This is a public-query observation, not an accounting proof from a certified state certificate. Recheck before upgrade.

At Finalized Base block 51015510, production Timelock `0x27fb581da2e58cd7fd9d22ddb0ee121dd55cbbb3` had the same initial/current proposer and executor, `0xf6dcc3fcde91c6ef3c5d73c94c48c58c7ff2cf84`. Paginated retrieval of all events since deployment block 50698194 reconstructed the role set, confirming no rotation events or extra executors. This defect does not require replacing production.

## FeePayout investigation

KINIC Ledger `73mez-iiaaa-aaaaq-aaasq-cai` reports public `git_commit_id` `cf41372e3d4dc1accfe2c09a7969f8bddc729dc1` and fee 100000 in `icrc1_metadata`. The following official DFINITY sources were inspected at that revision. Source was identified from metadata; reproducible equality with the Ledger Wasm was not verified.

- [Transfer entry point](https://github.com/dfinity/ic/blob/cf41372e3d4dc1accfe2c09a7969f8bddc729dc1/rs/ledger_suite/icrc1/ledger/src/main.rs#L540): transfer-state updates complete synchronously before the first archive await. Normal-transfer BadFee checks occur around L645, before deduplication.
- [Ledger transaction](https://github.com/dfinity/ic/blob/cf41372e3d4dc1accfe2c09a7969f8bddc729dc1/rs/ledger_suite/common/ledger_canister_core/src/ledger.rs#L214): after expiry/future-time checks, an identical transaction hash returns Duplicate before balances update. Normal InsufficientFunds occurs after deduplication.

If the initial call committed and is awaiting an archive response, a retry with fixed fee and identical identity within the deduplication period returns Duplicate. This normal-transfer implementation shows no path where insufficient balance hides that success. Deduplication expiry returns TooOld, which the Bridge classifies Ambiguous and requires absence reconciliation.

However, if the Ledger fee changes after initial success, a retry can return BadFee before deduplication. Bridge FeePayout releases its reservation on DefinitiveFailure from ReconciliationHold, potentially losing accounting of the earlier success under this condition. This is a concrete boundary depending on the fixed-Ledger-fee external assumption; the general rule that a definitive error proves earlier requests also failed is invalid.

Reading synchronous Ledger functions alone does not establish a general guarantee for delivery order when the first call has not executed or for multiple requests with later balance changes. This review changes no accounting behavior. FeePayout hardening would be a separate change allowing release from uncertain state only on success/Duplicate or complete absence evidence.

## Interpreting validation

Local shared-kernel proofs, production-adapter regression tests, and Lean abstract histories are distinct evidence. GlobalHistory does not directly model deadlines, so concrete reservation-release/refund histories do not establish general bounded liveness. Preserve existing external assumptions and feasibility premises.

Two Lean negative fixtures were updated for Record flag separation. Preserve their false propositions and update only trusted fixture hashes to the new contents. Updating hashes before running proofs is not evidence that negative fixtures passed.
