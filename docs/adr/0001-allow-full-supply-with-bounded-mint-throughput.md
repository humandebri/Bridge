---
status: accepted
---

# Allow full-supply transfers with per-Deposit and mint throughput limits

The Bridge accepts the possibility that the entire SNS token supply moves to Base, so it imposes no total cap on Bridge Exposure. Safety controls are the Per-Deposit Limit and Mint Throughput Limit enforced by the non-upgradeable Base contract. Since a per-request limit can be bypassed by splitting requests, aggregate minting over a short period is also limited.

## Considered Options

- Reject capping Bridge Exposure at a fraction of SNS total supply because it conflicts with allowing full-supply transfers.
- Reject a Per-Deposit Limit alone because consecutive requests would bypass the intended damage bound.
- Adopt both a Per-Deposit Limit and a Mint Throughput Limit, allowing the full supply to move over time.

## Consequences

- Limits apply only to new Deposit mints. A Withdrawal becomes `Committed` when burned on Base and has no refund mint or re-mint path.
- Apply the Per-Deposit Limit to each Deposit and accumulate new mint amounts within the same fixed window against the shared Mint Throughput Limit.
- Define limits in raw units; do not use token-decimal display conversions in safety decisions.
- Use Verus to prove that each Deposit respects the per-request limit and that mint throughput accounting is preserved, including reserved amounts.
- This decision does not guarantee a cap on cumulative transfers or Bridge Exposure. It controls the rate of damage from short-lived failures, compromise, or input errors.
