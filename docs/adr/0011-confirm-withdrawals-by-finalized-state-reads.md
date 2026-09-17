---
status: accepted
---

# Ingest Withdrawals through browser notification and a single synchronous verification

The browser discovers a Finalized `WithdrawalCommitted` event and sends its transaction hash to `notify_withdrawal` using a notification-only Identity persisted per deployment. The Canister requires an exact 2-of-3 receipt match and selects the second-highest Finalized height among three providers as the checkpoint. It retrieves the hash at that height by exact 2-of-3 agreement, performs a canonical probe of the receipt block, and binds the event, `getWithdrawal`, and Bridge snapshot to the checkpoint hash. Only after verifying the fixed quote and exact IC Account match does it begin the Ledger transfer.

Do not perform periodic discovery across all blocks. Retain browser Finalized monitoring every 15 seconds, but limit automatic notifications to the initial attempt, one short retry after disconnection or `Busy`, and one additional attempt when the head advances after `TransactionNotConfirmed`. After that, restore the saved transaction hash and failure reason in Progress or History and require explicit `Retry IC notification`. Any non-anonymous Principal may notify; the caller cannot change the recipient or amount. Stop if fewer than two providers succeed, the selected checkpoint's canonical hash fails to converge by 2-of-3, or receipt/state observations disagree. Do not fall back to Safe.
