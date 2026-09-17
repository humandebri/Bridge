---
status: superseded
---

# Split Base admin authority into immediate pause and delayed recovery

The human-wallet configuration in this ADR was replaced by Plan 006's SNS-centered, Canister-operated authority model. The current configuration has no human Base Admin/Canceller wallets; the Bridge Canister derives the Governance Operator, Runtime Administrator, and Independent Canceller on separate paths.

Fix Base contract mint limits and window duration at deployment. The Runtime Administrator can immediately pause and change the Service Fee within the cap. Only the Base Admin hardware wallet can unpause and rotate roles through a timelock, with cancellation assigned to a separate hardware wallet. This decision supersedes ADR 0007 and does not introduce a Governance Executor.

## Considered Options

- Do not revive SNS Governance approval with a Governance Executor adapter (ADR 0007). It requires a dedicated EVM-signing canister and a maintained fixed allowlist on ICP, imposing implementation and audit scope disproportionate to administration frequency.
- Reject making all Base contract parameters and roles immutable without an admin: Bridge Signer rotation would be impossible, permanently stopping the Bridge if canister reinstall changes its threshold ECDSA address.
- Reject direct Bridge administration by a single admin key because a leaked key would permit immediate unpause and role rotation.
- Reject having the operational Bridge canister also sign admin transactions; ADR 0007's reason still applies: Bridge compromise would remove Base safety constraints.
- Adopt separation of immediate pause and Service Fee changes through the Runtime Administrator from delayed recovery through a timelocked Base Admin wallet.

## Consequences

- Limit the Runtime Administrator's Base permissions to pause and Service Fee changes within `MAX_SERVICE_FEE`.
- Base Admin unpause and role rotation must wait for the timelock (initially 72 hours).
- The Canister-derived Governance Operator is the Timelock proposer/executor; a separately derived Independent Canceller holds the canceller role. Generic role changes remain frozen. Delayed self-calls may atomically rotate these operational members; the Bridge Timelock address remains immutable. Production delay remains at least 24 hours.
- Expose no selectors to change the Per-Deposit Limit, Mint Throughput Limit, or window duration.
- The Base Admin wallet is an operator outside SNS Governance. The UI and documentation must state this asymmetry with the ICP trust authority (SNS Governance in ADR 0008).
- Even Base Admin cannot change values declared immutable, such as `MAX_SERVICE_FEE`.
- Base Admin has no authority over minting, refunds, or escrow assets.
- Follow ADR 0016 for the concrete Base Admin Timelock configuration.
