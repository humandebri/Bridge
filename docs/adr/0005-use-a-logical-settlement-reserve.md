---
status: accepted
---

# Logically reserve settlement cycles and mint capacity

The Bridge shares one canister cycles balance. Logically reserve the conservative maximum cost of nonterminal liabilities so new Deposits cannot consume cycles needed to settle existing operations. Reserve Mint Authorization capacity separately only while `AuthorizationPending` or `AuthorizationAvailable`. The Base control plane separates signers, nonce lanes, and ETH balances for the Governance Operator, Runtime Administrator, and Independent Canceller, checking the required liability for each candidate transaction. User wallets pay Base gas for Deposit mints, so it is not part of an ETH reserve for Deposit admission.

## Considered Options

- Reject physically separating Deposit and Settlement canisters because it adds fund transfers, monitoring, recovery, and authority management.
- Reject unconditional sharing of cycles and mint capacity because new Deposits could consume execution resources needed by existing operations.
- Adopt logical reservation within the shared cycles balance, mint capacity reservations, explicit record-specific operations, and physical separation of Base control-plane roles.

## Consequences

- Settlement processes only records specified by a user or administrator.
- If the Settlement Reserve cannot be met, stop admitting new Deposits before pulling from the ICP ledger.
- The required settlement cycles reserve includes the operating floor plus the conservative maximum cost of every nonterminal liability.
- Reserve mint capacity only during `AuthorizationPending`/`AuthorizationAvailable`, releasing it on mint confirmation or transition to `RefundAvailable` after the deadline. Do not reserve ETH for Base mint gas.
- Sign a Base control-plane transaction only if the selected sender role's conservative Finalized and Safe ETH balances cover the candidate liability. Subsequent relay submits the fixed transaction. There is no separate fixed ETH floor.
- When the balance cannot support continued Withdrawal admission, pause new Withdrawals on the Base contract and continue only existing Settlements.
- Use Verus to prove that Deposit admission does not consume the Settlement Reserve.
- Audit upper-bound estimates for gas prices, EVM RPC costs, and management canister call costs as external assumptions.
- Logical reservation does not provide physical isolation against a malicious canister upgrade.
