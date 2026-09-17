---
status: accepted
---

# Use wallet-submitted EIP-712 Mint Authorizations

The Canister does not create or submit Deposit mint transactions. It threshold-ECDSA-signs an EIP-712 `MintAuthorization` bound to a Finalized Base snapshot. Any Base wallet may submit `mintDepositWithAuthorization` and pay the gas. The signature fixes the recipient.

An Authorization has a fixed 900-second lifetime from `issued_at_timestamp` in IC consensus time; never reissue a changed digest or deadline for the same Deposit ID. Finalize the Service Fee exactly once only if at least 300 seconds remain when the threshold signature is installed. Once the existing Finalized snapshot strictly exceeds the deadline, release the mint reservation without a per-Deposit Base reconciliation and enter `RefundAvailable`. This paragraph replaces the previous two-hour contract measured from a Finalized timestamp.

This decision removes mint ETH reserves, gas estimation, nonces, raw transactions, rebroadcast, replacement, and post-success IC wallet confirmation signatures. The UI combines the Base receipt/event with the Canister Deposit to display success. Only `request_deposit_refund` initiates refunds; any non-anonymous Principal may advance them, but the recipient, amount, and transfer identity are fixed in the Deposit record. If an Authorization was issued, verify `isDepositProcessed` at a canonical Finalized block. Refund only unprocessed Deposits; for processed ones, save the exact event/receipt and advance to `Minted`. On disagreement, fail closed without moving funds. There is no automatic Base reconciliation or automatic Ledger refund.

UI transaction confirmation, mint success notification, and history lists follow [ADR 0028](0028-confirm-transactions-and-list-recorded-history.md). Success notification is separate from requesting a refund.

UI pre-submission validation does not require 300 seconds remaining; it rejects when the latest Base timestamp exceeds the deadline. Preserve the Canister's 300-second condition at signature installation and Solidity's latest-timestamp-plus-900-second upper bound. Never extend an already-issued signature's deadline.
