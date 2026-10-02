# KINIC Bridge fee payouts

The Bridge records earned KINIC fees in `fee_reserve`. Its Ledger balance also includes assets held for bridge users and must never be used as a fee estimate. These operations retain stable schema v36 and the configured `bridge-production-fee-recipient` account. Receiving funds does not grant permission to request a payout: only the configured SNS Governance principal can execute payouts.

## Read the available amount

Use the repository-pinned Node.js and locked dependencies:

```sh
node tools/sns-proposal/fees.mjs fee-status
node tools/sns-proposal/fees.mjs fee-payout-status 7
```

Both public queries permit anonymous callers. The CLI verifies IC query signatures for the Bridge, KINIC Ledger decimals, and SNS registry. It uses the fixed production IDs: Bridge `lb5i5-ziaaa-aaaar-qcgwq-cai`, Governance `74ncn-fqaaa-aaaaq-aaasa-cai`, and Ledger `73mez-iiaaa-aaaaq-aaasq-cai`.

`get_fee_status` returns `fee_reserve`, `pending_payout_debit`, `ledger_fee`, `max_payout_amount`, `fee_recipient`, and `next_fee_payout_id`. The pending debit includes each reserved payout's amount **and** transfer fee. New capacity is `fee_reserve - pending_payout_debit - ledger_fee`, floored at zero for ordinary insufficient funds. A pending debit greater than the reserve or a storage read failure returns an error. Integer arithmetic uses the production shared kernel and never overflows at u128 boundaries.

`get_fee_payout(id)` returns an optional record with its fixed amount, recipient, original transfer fee, and state. `Succeeded { block_index }` records Ledger settlement; `Pending` and `ReconciliationHold` do not. An absent ID is distinct from a failed read.

## Prepare a separate DAO proposal

Amounts are canonical integer **raw units**, bounded by u128. The CLI also renders KINIC using the authenticated Ledger decimals without floating point conversion:

```sh
node tools/sns-proposal/fees.mjs prepare-fee-payout 100000000 payout-7.json
node tools/sns-proposal/fees.mjs prepare-continue-fee-payout 7 continue-7.json
```

Preparation never submits proposals. Output files are created exclusively and are not overwritten. Review the amount, recipient and subaccount, Ledger fee, payout ID, target and validator methods, topic, payload bytes, and SHA-256 hash. The amount, recipient and ID are fixed in the Candid payload. Re-read fee status before submission: a balance reduction, recipient change or conflicting use of the proposed ID can make a voted proposal fail safely at execution.

If the dedicated function is absent, the file includes an `AddGenericNervousSystemFunction` proposal. Its topic is `TreasuryAssetManagement`. The allocated ID excludes both active and reserved SNS IDs. Submit registration only after reviewing that concrete proposal. After execution, independently verify the registry's target canister, validator canister, method names and topic, then prepare the execution proposal again against that registry. Do not submit both generated registration and execution without this intervening check.

The function pairs are:

| Target | Validator |
| --- | --- |
| `sns_request_fee_payout` | `validate_sns_request_fee_payout` |
| `sns_continue_fee_payout` | `validate_sns_continue_fee_payout` |

Validators are read-only in behavior and reject invalid current state. At execution, the target checks Governance authority, sealed operational lifecycle, positive bounded amount, recipient and payout identity again. A new request must use the next available ID and fit the current capacity. An identical existing request reuses its record without reserving again; conflicting content fails. Continuation binds the existing record's amount and recipient and uses its original transfer identity. Permanently failed payouts cannot be continued; successful payouts are idempotent.

An SNS execution error produces an **IC reject**, including a stopped continuation. It does not return an application `Err` as a successful IC reply. Rejecting after an await preserves committed asynchronous progress. A successful request means a reservation was accepted; a successful continuation can mean reconciliation advanced. Neither alone establishes settlement. Check `get_fee_payout` until `Succeeded` and verify its Ledger block and recipient. Existing retry limits, deduplication, history reconciliation and accounting apply.

## Release and approvals

Implementation and proposal preparation do not authorize a production upgrade, UI publication, function registration or real transfer. Each needs review and approval of its concrete content.

Before merging, run the lightweight checks, affected proof-stage union and transaction tests. A release candidate requires `scripts/ci-local.sh all` from clean fixed source. Before preparing an SNS upgrade, observe the certified current module, Root-only controller, authenticated current v36 lifecycle/configuration/runtime binding, storage integrity and complete history indexes together. Reproduce the clean current HEAD Wasm twice after a complete current-source proofs receipt and bind the certified module immediately before submission. Any module or state change fails closed.

After the approved upgrade executes, verify fee reserve, reservations, recipient and existing payout records are preserved; exercise both public queries; verify runtime binding and the upgrade terminal. Update and validate the existing UI runtime profile for the new certified module hash before separately approved publication. No fee display is added to the UI. Register the new functions through separately approved proposals, verify the live registry, and prepare any actual payout as another proposal.

Direct claims are `fee_payout`, `operational_config_seal`, and `sns_fee_payout_authorization`. Abstract contracts, production shared kernels, Verus negative fixtures, Lean refinement vectors, adapter tests and SNS transaction tests are registered in the proof manifests. External boundaries remain partial: authentic ICRC history, SQLite atomicity, IC message caller authenticity and runtime/toolchain semantics are assumptions, not unconditional end-to-end proofs. Ledger fee immutability remains an operational requirement of the existing transfer implementation.
