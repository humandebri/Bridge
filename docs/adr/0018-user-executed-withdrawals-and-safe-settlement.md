---
status: accepted
---

# Treat Withdrawals as irreversible Committed burns

The user submits `createWithdrawal(amount, maxServiceFee, owner, subaccount)`. Before burning, the contract verifies that the execution-time Service Fee is within the user's cap and `amount > serviceFee`, then atomically performs `transferFrom`, burn, and transition to `Committed` with fixed `amountOut = amount - chargedServiceFee`.

`Committed` is terminal on Base: there is no Base refund, release acknowledgement, or cancellation. The Canister verifies the canonical Finalized receipt, event, state, and snapshot at the same block hash, storing a fixed amount, IC Account, and transfer identity as a liability. Each `continue_withdrawal` by any non-anonymous Principal advances Ledger transfer or reconciliation by at most one external step; timers do not retry it. The Bridge pays the Ledger Fee, with its immutability at 100,000 raw units an external assumption. If that fixed Ledger Fee exceeds the charged Service Fee, do not create a release; save the Observed record, fixed-fee guard, and audit event. Operators pause Base Withdrawals and check configuration; the same record resumes only after successful revalidation through `continue_withdrawal`. Runtime settlement does not query `icrc1_fee()`. `BadFee` for Withdrawals or Deposit refunds must not change the amount, fee, or transfer identity.

## Consequences

- A normal Withdrawal requires only one Base transaction, Finalized confirmation, and user confirmation.
- Remove per-Withdrawal threshold ECDSA signing, a second gas payment, nonce, confirmation job, and EVM recovery.
- Resolve Ledger failures by retrying and reconciling history with the same Withdrawal ID, IC Account, and transfer identity. Administrators cannot change recipients or make arbitrary payouts.
- Withdrawal review displays the amount, fee, and IC recipient and revalidates the fee, balance, wallet, and chain before signing. First use per deployment requires a general risk acknowledgement for an unaudited bridge; the final `createWithdrawal` is approved by wallet signature. No Withdrawal-specific burn/no-Base-refund warning or checkbox is required. Accept this simplification as a product decision: it adds no Base refund, release acknowledgement, or cancellation and does not change the irreversible `Committed` design.
- Stop if the Finalized head and canonical hash fail to converge by 2-of-3; do not fall back to Safe or a fixed confirmation count.
