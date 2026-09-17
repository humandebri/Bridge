---
status: superseded
---

# Acknowledge ICP Release on Base

Superseded by ADR 0018. Base burn becomes an irreversible `Committed` state, and subsequent ICP transfers become Canister liabilities. Remove `acknowledgeRelease`, `cancelRelease`, and `refundWithdrawal` from the ABI.

This removes the second EVM transaction, threshold ECDSA signature, gas cost, and Finalized confirmation for each Withdrawal. In exchange, no Base refund is available after burn; the Canister retries and reconciles Ledger transfers using the fixed Withdrawal ID, IC Account, and amountOut.
