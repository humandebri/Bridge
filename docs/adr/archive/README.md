# ADR archive

This directory contains non-normative decision history superseded by later ADRs. Do not use it as the source of truth for implementation, operations, or review.

## Deposit confirmation

- [ADR 0017: Automate settlement confirmation](deposit-confirmation/0017-automate-settlement-confirmation.md)
- [ADR 0020: Start Deposit EVM confirmation through frontend notification](deposit-confirmation/0020-use-wallet-confirmed-frontend-evm-confirmation.md)

The `MintDeposit`, `confirm_deposit`, and Canister-originated mint transactions assumed by these records have been removed. [ADR 0023: Use wallet-submitted EIP-712 Mint Authorizations](../0023-use-wallet-funded-eip712-mint-authorization.md) is the source of truth for current Deposit minting.
