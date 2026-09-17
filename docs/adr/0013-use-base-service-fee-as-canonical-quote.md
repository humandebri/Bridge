---
status: accepted
---

# Use the Base Service Fee as the canonical quote

The Base contract state is the source of truth for the operating Service Fee; the Bridge canister reads it at a Finalized block. The Runtime Administrator may change it only within immutable `MAX_SERVICE_FEE`. Deposit and Withdrawal `maxServiceFee` protect users during changes, so these changes do not require the Base Admin timelock. This decision adds Service Fee changes within the cap to ADR 0009's list of Base-side Runtime Administrator permissions. Withdrawal `amountOut` is fixed at execution on Base.
