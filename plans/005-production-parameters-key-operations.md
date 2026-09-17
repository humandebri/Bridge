# Plan 005: KINIC production parameters and emergency pause demonstrations

## Status

- **State**: IN PROGRESS
- **Initial activation evidence**: exact schedule/execute gas estimates, at least 10 distinct Finalized fee blocks, idle cycles burn, the actual pause principal, and fixed limits.
- **Post-unpause Gate C evidence**: pause/cancel drills, at least seven days of Base fee distributions, and at least 10 Governance gas/settlement cycles samples each. Do not automatically apply observations to operating configuration.

## Implemented locally

- Primary evidence for KINIC Ledger/Index/Root/Governance and finalized fee settings.
- Conservative parameter derivation and deployment-profile validation CLI.
- Mint limits immutable after deployment and Timelock configuration with a Canister-derived Governance Operator.
- Bridge request-time reserve gates, Safe observation timestamps, and manual pause APIs.
- Threshold signer replenishment, monitoring drills for one emergency pause principal, and runbooks.

Do not mark a mainnet candidate `validated` while initial activation evidence is missing. Collect Gate C evidence, including pause/cancel drills, separately as post-unpause operational evaluation; it does not authorize initial activation or controller handover. Require no release approver, finance principal, multiple pause principals, or human EVM administration keys.

Derive initial cycles values from paused `idle_cycles_burned_per_day` and the fixed settlement ceiling as follows.

```text
settlement cycle ceiling = 5,000,000,000 cycles
cycles floor = (idle cycles burn/day + 5,000,000,000) × 30 × 2
```

Initial fee evidence uses exact schedule/execute calldata estimates and at least 10 distinct Finalized blocks. Set the gas limit to 130% of the maximum estimate rounded up to 1,000, priority fee to p95×4, max fee to base fee p99×20, and L1 ceiling to p99×10. Fix quote validity at 90 seconds and multipliers at 13,000/60,000/15,000 bps. Do not create or submit transactions when fee caps are exceeded or cycles are insufficient. Do not derive the Mint Throughput Limit or Per-Deposit Limit here; fix approved raw values in the profile based on 5/15/60 monitoring targets and maximum acceptable damage. Both successful pause/cancel paths and meeting 5/15/60 belong to post-publication Gate C operational evaluation.
