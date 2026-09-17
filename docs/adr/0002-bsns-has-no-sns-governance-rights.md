---
status: accepted
---

# Give bSNS no SNS Governance rights

Restrict bSNS to a circulating token on Base backed 1:1 by Bridgeable SNS Tokens, without SNS Governance voting rights, neuron permissions, or voting rewards. Users wishing to participate in Governance must burn bSNS, release SNS tokens on ICP, and stake them in a neuron.

## Considered Options

- Reject cross-chain voting based on Base balance snapshots because voting rights during transfers, double-vote prevention, delegation, and balance locking during voting would require a new protocol.
- Reject proxy staking and voting of escrow balances by the Bridge canister because it cannot represent each user's intent and would concentrate excessive Governance authority in the Bridge.
- Adopt bSNS as a token with 1:1 backing and no voting rights.

## Consequences

- SNS tokens staked in neurons are not Bridgeable SNS Tokens. Users cannot deposit them until the neuron is dissolved and disbursed.
- Do not use SNS tokens in Bridge escrow for staking, voting, treasury investment, or lending.
- Before a Deposit, the UI must state that bSNS does not provide voting rights or voting rewards.
- Do not grant SNS neuron permissions to bSNS holders or introduce a Governance identity mapping between Base addresses and Principals.
- Bridge accounting and formal proofs cover 1:1 asset backing; cross-chain governance is outside the proof scope.
