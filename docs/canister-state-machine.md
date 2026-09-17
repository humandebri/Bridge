# Bridge canister state machine

## Persistence and execution boundaries

`bridge-core` defines deterministic state transitions independent of caller, time, ICRC Ledger, EVM RPC, Candid, and storage. `bridge-canister` persists state in a single SQLite database and connects the Ledger, EVM RPC, threshold ECDSA, administration APIs, and stable job executor.

Normal reopen accepts only stable schema v36 and record wire version v30. Only production and test-deployment `post_upgrade` accept the one-time atomic migration from deployed version 35/wire v30 to v36; all other old/unknown schemas, unknown wire formats, and undecodable databases fail closed and refuse startup. Current staging also accepts only upgrades preserving the same Canister and deployment instance; never replay or resume the one-time reinstall history.
Upgrade validation checks same-Wasm reopening with current-schema v36 records, configuration, quotas, and audit state preserved, activation evidence migration from version 35, and rejection of other old schemas/wire formats.

`settlement_jobs` is authoritative for running and stopped Settlements. Timers automatically claim Deposits and fee payouts; only explicit `continue_withdrawal` manually claims Withdrawals. Withdrawal notification atomically saves only the record and fixed transfer identity, without creating a job. Persist signature dispatch or Ledger transfer identity before external `await`; only lease generation and database state determine execution authority.

There is no Base mint transaction lane. The Governance lane persists only nonce, signed generation, raw transaction, and hash. The Canister does not broadcast, monitor receipts, rebroadcast, or automatically replace transactions; after submission, the external relayer notifies the Canister of the specified hash's Finalized result.

## Deposit (ICP → Base)

Deposit IDs are domain-separated hashes of `(canister ID, Base chain ID, Bridge address, deployment instance ID, caller, owner_sequence)`; a different payload with the same install domain and sequence causes `DepositConflict`. Admission saves a `Prepared` funding attempt, fixed transfer identity, consumed Deposit quota, and active funding reservation, and checks the cycle reserve. Persist `Dispatched` before the ICRC-2 pull. Only Ledger success or `Duplicate` permits paid Base preflight; promote funded assets even if that preflight fails so settlement can retry or refund them. Definitive Ledger failure removes the attempt and reservation and returns quota only within its original window. Unfunded attempts must not retain shared verification capacity.

```text
FundingAttempt
  ├─ Ledger success / Duplicate → EscrowedUnquoted
  ├─ Definitive Ledger failure → delete attempt (no formal Deposit; return same-window quota)
  └─ Unknown Ledger outcome → FundingReconciliationHold
                                ├─ Success evidence → EscrowedUnquoted
                                └─ Complete absence evidence → Cancelled

EscrowedUnquoted
  ├─ Finalized quote and capacity reservation → AuthorizationPending
  ├─ Pause/fee/limit rejection → RefundAvailable
  └─ RPC failure / observation disagreement → stopped (no refund)

AuthorizationPending
  ├─ Threshold ECDSA signature over the same digest → AuthorizationAvailable
  └─ Definitive failure before Authorization issuance → RefundAvailable

AuthorizationAvailable
  ├─ notify_deposit_mint with exact canonical Finalized evidence → Minted
  └─ Finalized timestamp exceeds deadline
       → RefundAvailable (reservation released; Base not reconciled)

RefundAvailable
  └─ Non-anonymous claim before Authorization issuance (no Base outcall) → RefundPending → Refunded

RefundAvailable
  ├─ notify_deposit_mint with exact canonical Finalized evidence → Minted
  └─ Non-anonymous claim after Authorization issuance
       ├─ Exact mint evidence → Minted
       ├─ Finalized unprocessed evidence → RefundPending → Refunded
       └─ Disagreement / missing evidence → fail closed (no fund movement)

RefundPending
  └─ Unknown Ledger outcome → RefundReconciliationHold
                         └─ Renewed claim by non-anonymous caller reconciles the same transfer
```

1. `EscrowedUnquoted → AuthorizationPending` saves the Finalized Base snapshot, quote, all Authorization fields, EIP-712 domain, digest, originating Finalized block number/hash/timestamp, mint capacity reservation, and job in one SQLite transaction.
2. Use the Finalized Base snapshot only for state, fees, pause, and refund evidence. Determine the deadline exactly once by checked addition of a fixed 15-minute (900-second) TTL to `issued_at_timestamp` in IC consensus time; it is not computed by adding to the Finalized timestamp.
3. Save the dispatched flag and attempt number before threshold ECDSA `await`. After timeout, callback loss, or upgrade, re-sign only the same digest without changing deadline or payload. Normalize the 65-byte signature to low-s `r || s || v`; only when the recovered address matches the expected Mint Signer, credit the Service Fee exactly once to the fee reserve and publish the signature in the same transaction.
4. In `AuthorizationAvailable`, any Base wallet may submit the signed payload to the contract and pay gas. The Canister does not track transactions or receipts during this period.
5. Permit signature installation only with at least 300 seconds remaining in IC consensus time. Accept exactly 300 seconds; at 299 seconds or less, do not install the signature or accrue the Service Fee, and wait for Finalized unprocessed evidence. The deadline-ordered index using Base Finalized snapshots from new Deposits or other operations treats only `finalized_timestamp > deadline` as expired, retaining reservations at equality.
6. While backlog remains, conservatively overcount unprocessed reservations. Return a retryable error if new admission limits cannot be evaluated accurately; never undercount. Without new Deposits, no additional reservation capacity is consumed, so there is no expiry timer.
7. Any non-anonymous Principal may advance a refund with `request_deposit_refund`. Recipient, amount, and transfer identity are fixed in the existing record, not caller inputs. Before Authorization issuance, `RefundAvailable` sends `gross - refund ledger fee` without a Base outcall. The initial ICRC-2 pull fee remains paid by the wallet and is not returned.
8. For `RefundAvailable` after Authorization issuance, use EIP-1898 to bind runtime identity, signer, epoch, strict deadline, and `isDepositProcessed(depositId)` to the same canonical Finalized block hash. Only `processed == false` permits refunding `gross - charged service fee - refund ledger fee`. The Service Fee, initial pull fee, and refund fee are not returned.
9. If `processed == true`, retrieve `DepositMinted` from the originating block through the observed Finalized head. Verify exactly one event, contract, digest, recipient, amount, fee, and canonical successful receipt before advancing to `Minted`. Missing/multiple/mismatched events, RPC disagreement, stalled Finalized progress, or runtime mismatch must not move funds.
10. Retain the same transfer identity in `RefundReconciliationHold` for unknown Ledger results. Do not retry by timer; each renewed claim from any non-anonymous caller advances one reconciliation step. Treat Duplicate as success of the same transfer; issue no new identity without complete absence evidence.
11. Pause, unpause, epoch changes, and signer rotation invalidate unexpired Authorizations created before each transition on the contract, but do not justify early refunds. Always require the original deadline and Finalized unprocessed evidence.

Reserve unprocessed Authorizations as mint-window liabilities until deadline expiry is observed. Deposit admission does not depend on Mint Signer ETH, gas prices, or nonces. Retain the cycles floor and settlement cycle ceiling for signing and RPC/Ledger processing during explicit refunds.

## Withdrawal (Base → ICP)

For a Withdrawal, the Base wallet submits `createWithdrawal`, atomically performing bSNS `transferFrom`, burn, and creation of a `Committed` record with a fixed payout. The Canister does not create the transaction.

```text
Base Committed
  → notify_withdrawal(transaction_hash)
  → Verify canonical Finalized receipt, event, state, and snapshot
  → Observed → ReleasePending (no automatic job)
  → continue_withdrawal (1 call 1 external step)
       ├─ Success → Paid
       └─ ReconciliationHold
            ├─ Success evidence → Paid
            └─ Complete absence evidence → ReleasePending with new identity (transfer on next call)
```

The UI saves the transaction hash and notification attempt state in v7 localStorage format, automatically calling `notify_withdrawal` through a deployment-scoped browser identity only on initial Finalized event detection. Limit short retries after disconnection or `Busy` to one, and notifications after head advancement following `TransactionNotConfirmed` to one; other failures require explicit Progress or History actions. After success, call `continue_withdrawal` once with the same identity; nonterminal results move to History's `Continue payout`. Notification and continuation require no IC wallet signature or ICRC-21 consent. The Canister retains each provider's Finalized block number and hash, accepting only checkpoints with exact 2-of-3 agreement on both. Preserve the receipt's canonical probe and bind the event, `getWithdrawal`, and Bridge snapshot to the checkpoint hash. Do not treat an unknown Ledger result as failure merely because time passes; retain Hold until complete Ledger and Index watermarks prove absence.

## Public APIs and authority

| API | Caller | Responsibility |
|---|---|---|
| `request_deposit` | Deposit owner | Start Ledger pull and Authorization creation |
| `request_deposit_refund` | Any non-anonymous Principal | Check claimable amount, perform required Finalized reconciliation, execute fixed Ledger refund or reconcile Hold |
| `notify_withdrawal` | Any non-anonymous Principal | Permissionless Finalized Withdrawal notification; recipient bound to the Base event |
| `continue_withdrawal` | Any non-anonymous Principal | Advance fixed Ledger release or reconciliation by at most one external step |
| Base governance prepare/status/replace/confirm | Governance, or pause principal for pause/cancel only | Signed artifacts for the external relayer and Finalized confirmation |
| `prepare_next_emergency_base_action` | Governance, pause principal | Sign emergency queue pause/cancel actions in order |
| `get_deposit` / `get_deposit_by_owner_sequence` | Public query | Query Authorization, deadline, signature, and state |
| `get_bridge_status` | Public query | Query Finalized observations, epoch, Governance reserve, and scheduler |

The SNS Governance principal handles normal resume, principal rotation, Fee Recipient, fee payouts, Service Fee, and Timelock operations. During Bootstrap, only the current controller may seal, fixing that principal as the bootstrap activation controller. While internal bootstrap activation authority remains and the fixed principal remains controller, only that principal may perform initial paused schedule/execute and pending resume/replacement. Confirmation of the first execute permanently consumes the internal authority and transfers subsequent activation authority to the existing Governance principal, regardless of when external controller settings change. After every external await using controller authority, recheck that the management canister's controller set contains only the fixed principal; reject removal, addition, or replacement before commit. The pause principal performs only emergency pause and permitted progress. Mint Signer is dedicated to EIP-712 Authorizations and Governance Operator to Canister-originated Base governance transactions, with separate derivation paths and ETH management.

## Governance transaction affordability

Before signing, compute the checked transaction liability and read the Governance Operator balance at Finalized and Safe. The shared kernel returns `observed = min(finalized, safe)` and accepts exactly when `observed >= required`. Equality is affordable; either balance below the requirement rejects without consuming a nonce or changing a pending replacement generation. RPC authenticity and the adequacy of the configured fee ceiling remain external assumptions.
