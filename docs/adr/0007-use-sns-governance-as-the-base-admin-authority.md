---
status: superseded by ADR-0009
---

# Make SNS Governance the Base admin authority

SNS Governance is the Governance Authority approving Base contract limit changes, role rotation, and unpause. Do not add EVM implementation to the standard SNS Governance canister; use a Governance Executor solely to translate adopted custom proposals into EVM transactions.

## Considered Options

- Reject having the operational Bridge canister also sign admin transactions because Bridge compromise would then remove Base safety constraints.
- Reject direct EVM transaction construction and signing by SNS Governance because it changes the responsibilities and implementation scope of the standard SNS Governance canister.
- Adopt SNS Governance as the sole approval authority, with a dedicated Executor as a technical adapter.

## Consequences

- Register the Governance Executor's validate and execute methods as an SNS custom proposal.
- The execute method processes only calls from the configured SNS Governance principal. Reject anonymous callers and the operational Bridge canister.
- Place the Governance Executor under SNS Root control.
- A threshold ECDSA address dedicated to the Governance Executor holds the Base contract's `DEFAULT_ADMIN_ROLE`.
- Restrict the Governance Executor's targets, chain IDs, contract addresses, and function selectors to a fixed allowlist; prohibit arbitrary calldata forwarding.
- The operational Bridge canister and Governance Executor have different canister principals and cannot sign as each other's threshold ECDSA addresses.
- Use Verus to prove the correspondence among caller authorization, the allowlist, proposal payload validation, and admin transaction construction.
- SNS Governance is the authority and the Governance Executor is an execution adapter; do not introduce an independent admin organization or multisig.
