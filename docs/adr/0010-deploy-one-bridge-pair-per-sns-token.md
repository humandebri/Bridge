---
status: accepted
---

# Deploy each Bridge for a single SNS token

Dedicate each Bridge canister and Base contract pair to a single SNS token; do not handle multiple SNS tokens in one canister. ADR 0008 completes handover only when the corresponding SNS Root is the sole controller. A canister shared by multiple SNSs cannot be handed over to any one SNS Root and is incompatible with that design.

## Considered Options

- Reject a multi-tenant canister for multiple SNSs because upgrade authority cannot belong to one SNS Governance, and pauses, Settlement Reserves, and cycles balances would interfere across SNSs.
- Adopt an independent canister/contract pair for each SNS.

## Consequences

- Remove token-ID branching from state and deployment configuration.
- Confine Settlement Reserve, fee reserve, and Reconciliation Hold accounting to one token.
- Write reusable code independent of the SNS, but do not introduce a factory until needed.
- Expansion to multiple SNSs requires separate handover, Verus proofs, and audits for each pair.
