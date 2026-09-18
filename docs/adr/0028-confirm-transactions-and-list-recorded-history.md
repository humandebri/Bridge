---
status: accepted
---

# Confirm individual transactions and list Canister-recorded history

Normal UI submission and history display do not scan ranges of Base events. After submission, inspect the receipt for the saved transaction hash and verify the target contract, event, and match with the authorization or call arguments. Display successful mint execution as `Success`, treating finality and Canister recording as separate states. Separate Base Withdrawal success from completed IC payment. Communication failures must not erase confirmed success; return to rechecking only when canonicality disagreement or receipt disappearance is established.

`notify_deposit_mint` accepts a deposit ID and transaction hash. The Canister independently verifies fixed-RPC quorum, runtime, a canonical receipt bound to a finalized checkpoint, and an exact authorization match. Pass the evidence to the existing `MintReconciled` production kernel for recording. Frontend success reports are not evidence, and notification does not initiate refunds. Existing refund requirements for deadline, unprocessed state, and finalized evidence remain unchanged.

Deposit lists use the existing API, adding the saved mint receipt's transaction hash, block number, and log index to public views. Withdrawal lists use a per-Base-requester index, not the notifier's Principal as user identity. Pages default to 20 records and allow at most 100, with stable cursors combining descending notification acceptance time and Withdrawal ID. Current Withdrawal records are not deleted, so history_truncated is false. Future retention changes must delete the record, notification index, and user index in the same transaction and update truncation information.

Add indexes in the migration from v35 to the then-unreleased v36. Preserve SQLite MemoryId 120 and wire version 30. Build reverse lookup for existing notifications and per-user indexes for existing Withdrawals in synchronous transactions of at most 100 records each, persisting each batch cursor. Return IndexNotReady during rebuilding. Do not publish the new UI until the Canister upgrade and index verification are complete; do not weaken evidence constraints for live v35.

Track unnotified transactions in device-local resume data. On another device, recover by entering the transaction hash and comparing the receipt against the connected sender/recipient. Do not automatically discover unnotified transactions on another device without a known hash. Add no shared indexer or external Explorer API.

Specify UI RPC in the verified runtime profile. Restrict the Alchemy UI key's allowed Origin to https://bridge.kinic.xyz; do not change Canister RPC keys or settings. Origin restrictions constrain use of a public browser key; they do not make it secret. Do not use production keys in test environments.

## Evidence and validation scope

State-transition safety follows the existing production kernels for authorization_binding, exact_mint_finalization, expiry_refund, refund_evidence_enforcement, and withdrawal_finalization. Reuse notification_quota_isolation for notification resource limits. Bind UI resume data to pending_queue. Regression tests verify the new RPC receipt adapter and history index integration, but abstract theorems alone do not prove the entire SQL index implementation. RPC canonicality, fixed-provider chain binding, cryptographic authenticity, IC message atomicity, and browser persistence availability remain external assumptions.

The publication UI verifies authenticated current v36 state. Bind the certified current module/schema and explicit UI RPC configuration digest to the runtime profile, requiring exact agreement with live RuntimeBinding immediately before deployment. Render Alchemy URLs deterministically only from this public configuration. During index rebuilding, `list_withdrawals` failure must reject the publication gate. Provide no compatibility path for publishing against another schema or module.
