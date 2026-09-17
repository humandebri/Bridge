---
status: accepted
---

# Charge a fixed SNS-token fee at Bridge request finalization

Charge a fixed Service Fee in SNS tokens for each Deposit and Withdrawal, independent of the transfer amount. A Deposit's fee is finalized when the signed Mint Authorization is persisted; a Withdrawal's fee is finalized when the Withdrawal is established on Base. Bridge costs primarily depend on request counts, so percentage fees are not used. The current Service Fee may change but cannot exceed the immutable `MAX_SERVICE_FEE` fixed in raw units at deployment.

This revision replaces the earlier statement that a Deposit fee is finalized only after successful Base minting. With wallet-submitted Mint Authorizations, issuance means the Canister has completed signing, RPC, and persistence work and has given the user the ability to submit a mint before expiry.

## Considered Options

- Reject percentage fees because they impose excessive costs on large transfers and conflict with allowing full-supply transfers.
- Reject combining fixed and percentage fees because it adds rules for fee caps, rounding, and minimum amounts.
- Adopt a fixed fee for each request reaching the finalization boundary above.

## Consequences

- The Base mint amount for a Deposit equals the amount locked on ICP minus the Service Fee.
- The ICP payout for a Withdrawal is the fixed `amountOut`, equal to the amount burned on Base minus only the Service Fee. The Bridge pays the Ledger Fee.
- Deposit `max_service_fee` and Withdrawal `maxServiceFee` protect users from fee changes during processing. Withdrawal `amountOut` is fixed at execution as `amount - chargedServiceFee`.
- Service Fee changes must satisfy `MIN_SERVICE_FEE <= service_fee <= MAX_SERVICE_FEE`; `MAX_SERVICE_FEE` itself is immutable.
- Both the Base contract and Bridge canister reject fee changes above the cap.
- A Deposit Service Fee is credited to the fee reserve exactly once, in the same SQLite transaction that saves the signed Mint Authorization. It is not finalized before signing.
- A finalized Deposit Service Fee is not returned even if the issued Authorization is never submitted, reverts, expires, or is refunded after expiry with evidence that it was unprocessed.
- A Withdrawal Service Fee remains finalized when the Base Withdrawal is established; ICP Release retries must not count it again.
- Account for the fee reserve separately from Bridge Exposure backing.
- Administrators may change the Fee Recipient. Record the change in events and audit logs.
- A Fee Recipient change assigns the entire unpaid finalized fee reserve to the new recipient.
- Do not retain per-recipient fee buckets or balances for former recipients.
- Fee payouts may use only the finalized fee reserve and cannot spend assets backing Bridge Exposure.
- Authority to change the Fee Recipient or Service Fee does not include authority to mint, re-mint Withdrawals, or make arbitrary transfers.
- Do not automatically convert SNS-token fees into ETH for Base gas. Maintain a separate procedure for operators to replenish ETH.
- Use Verus, Lean, Solidity SMTChecker, and transaction tests to verify Service Fee caps, prohibition of Deposit fee finalization before Authorization signing, no double accounting at each finalization boundary, reserve preservation on recipient changes, and prohibition of payouts exceeding the fee reserve.
