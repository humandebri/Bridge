---
status: accepted
---

# Support EIP-3009 authorized transfers in bSNS

Keep bSNS ERC-20-compatible and implement EIP-3009 authorized transfers for direct use in x402 `exact` payments on Base.
Standard ERC-20 with Permit2 can also support x402 payments, but bSNS itself should provide a path where a facilitator settles a single user signature without an external proxy.
Because bSNS is non-upgradeable and adding features later requires redeployment, adopt this before freezing the Phase 1 interface.

## Considered Options

- Do not make standard ERC-20 plus Permit2 the only path: it avoids additional token implementation but makes x402 payments depend on an external Permit2 contract and proxy.
  Standard ERC-20 allowances remain available, so Permit2 can still be used as an alternative.
- Reject adding only ERC-2612 because it sets allowances by signature and does not provide x402's direct EIP-3009 transfers.
- Reject adding ERC-721 because it defines nonfungible ownership transfers and does not match bSNS's 1:1 raw-unit backing of an ICRC-1 token.

## Consequences

- bSNS exposes `transferWithAuthorization`, `receiveWithAuthorization`, `authorizationState`, and `cancelAuthorization`, recording authorization use and cancellation in events.
- The EIP-712 domain binds signatures to the token name, fixed version `"1"`, execution chain ID, and bSNS contract address.
  Reject signatures created for another chain or contract.
  Expose the fixed version through `version()` and the full domain through EIP-5267 `eip712Domain()`.
- Manage authorization nonces in a single namespace per authorizer.
  Used or cancelled nonces cannot be reused by any authorized transfer function.
- `receiveWithAuthorization` requires caller and recipient equality, preventing a third party from reusing the signature to front-run only the transfer.
- Authorized transfers may move only existing balances; they add no mint or burn authority.
  Preserve the constraint that only the Bridge can change supply.
- Operating and checking compatibility of x402 resource servers and facilitators are outside the Bridge's responsibilities and do not block Bridge deployment or activation.

[EIP-3009](https://eips.ethereum.org/EIPS/eip-3009) is authoritative for its signature format and security considerations.
If x402 integration is provided separately, determine supported EVM tokens using the [x402 Network & Token Support](https://docs.x402.org/core-concepts/network-and-token-support) documentation current at integration time.
