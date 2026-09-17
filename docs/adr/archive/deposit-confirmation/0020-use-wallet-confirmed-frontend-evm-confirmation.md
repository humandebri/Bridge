---
status: superseded
superseded_by: ADR 0023
normative: false
---

# Start Deposit EVM confirmation through frontend notification

> **Non-normative historical document:** [ADR 0023](../../0023-use-wallet-funded-eip712-mint-authorization.md) superseded this ADR. The following describes the former Canister-originated mint transaction approach and must not be used as an implementation basis.

`MintDeposit` is the only Canister-originated EVM operation. The frontend observes the receipt and Finalized head, then calls `confirm_deposit` from an authenticated IC wallet. The Canister revalidates the saved transaction match and canonical Finalized inclusion through EVM RPC quorum.

Withdrawals have no Canister-originated EVM transactions, so there is no `confirm_withdrawal`, pending EVM confirmation, or timer fallback. `continue_withdrawal` advances only Ledger transfers or history reconciliation.
