---
status: accepted
---

# Implement the Mint Throughput Limit with fixed windows

Implement the Base contract's Mint Throughput Limit with fixed windows that reset at regular intervals (initially one hour), rather than a sliding window. The contract is non-upgradeable; a fixed window needs only one cumulative amount per window, minimizing state and SMTChecker proof scope.

## Considered Options

- Reject a sliding window: although it prevents bursts across window boundaries, it introduces mint history or an approximation structure into the immutable contract.
- Adopt fixed windows and account for boundary bursts when setting the limit.

## Consequences

- Up to twice the window limit can be minted in a short interval spanning a boundary. Derive the Mint Throughput Limit by dividing the maximum acceptable damage by two (`docs/parameters.md`).
- Fix window duration and the limit in the constructor; no authority may change them on an existing contract. Different values require deployment of a new Bridge pair with a separate safety review and activation.
