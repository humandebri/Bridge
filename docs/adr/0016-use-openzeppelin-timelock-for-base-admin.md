---
status: accepted
---

# Use OpenZeppelin TimelockController as the Base Admin execution boundary

Route Base Admin operations that increase risk through OpenZeppelin Contracts 5.6.1 `TimelockController`.
Deploy the Timelock before the Bridge with an initial minimum delay of 24 hours, the Canister-derived Governance Operator as proposer/executor, the separately derived Independent Canceller as canceller, and `address(0)` as additional admin.
Only the Timelock itself holds `DEFAULT_ADMIN_ROLE`. Generic grant, revoke, and renounce calls are frozen, including self-calls. A delayed self-call to `rotateOperationalMembers` is the sole exception: it atomically rotates the proposer/executor pair and independent canceller while preserving nonzero, distinct roles. The Bridge's Timelock address is immutable; replacing the Timelock contract is not an available recovery operation.

## Considered Options

- Reject a custom queue and time checks inside the Bridge because they expand the ABI and audit scope and duplicate an already verified implementation.
- Reject a permissionless executor: although it cannot alter scheduled operations, this configuration prioritizes restricting execution to the Canister-derived Governance Operator.
- Reject granting temporary admin authority to the deployer because it creates a period in which the delay can be bypassed.

## Consequences

- Direct calls from external EOAs to Bridge Base Admin functions fail; the Canister schedules through Timelock and executes after 24 hours.
- At construction, the Bridge verifies Timelock bytecode, `getMinDelay() >= 24 hours`, and the Timelock's own possession of `DEFAULT_ADMIN_ROLE`.
- Keep one Canister-derived Governance Operator as proposer/executor and one separately derived Independent Canceller. Grant no roles to human wallets. Governed recovery rotates these operational members through the delayed self-call, together with the Bridge Signer and Runtime Administrator in one batch.
