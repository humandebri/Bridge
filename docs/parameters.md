# Parameter derivation

This document records derivation formulas and initial values for Bridge safety parameters.
Define all values in raw units; do not use token-decimal display conversions for decisions (ADR 0001).
Fix mint limits and window duration at deployment; no authority may change them.

## Mint Throughput Limit

Fixed windows (ADR 0012) permit up to twice the limit to be minted in a short interval spanning a window boundary.

```
Limit per window = maximum acceptable damage ÷ 2
Maximum acceptable damage = acceptable loss during the expected time until monitoring triggers pause
```

- Initial window duration: one hour.
- Initial production limit: `15000000000000` raw (150,000 KINIC). Specify the identical value in the approved deployment profile and never change it after deployment.

Independently of window duration, monitoring must detect anomalies within five minutes, obtain operator acknowledgement within 15 minutes, and pause both Base and IC within 60 minutes.
If monitoring cannot meet these targets, reduce the limit rather than extending the window.

## Per-Deposit Limit

```
Per-Deposit Limit = fixed value in the approved deployment profile
```

- Initial production value: `15000000000000` raw (150,000 KINIC, approximately 2.5% of total supply as of July 17, 2026).
- The initial Mint Throughput Limit is also `15000000000000` raw, so one Deposit can consume the entire one-hour window.
- The short-interval maximum across a fixed-window boundary is two windows: 300,000 KINIC (approximately 5% of total supply).

Because the Mint Throughput Limit bounds aggregate volume, this value targets individual input errors and anomaly detection for a single request.

## MAX_SERVICE_FEE

Immutable; no authority can change it after deployment (ADR 0004).
Set it conservatively high to accommodate future price changes, starting the operating `service_fee` below it.

```
MAX_SERVICE_FEE = 10 KINIC
Initial service_fee = 0.5 KINIC
```

- KINIC ledger fee: `100000` raw
- Base Sepolia staging TICRC1 ledger fee: `10000` raw (`test-deployment` builds only).
- `MAX_SERVICE_FEE`: `1000000000` raw (10 KINIC; 10000 times the ledger fee).
- Initial operating `service_fee`: `50000000` raw (0.5 KINIC; 500 times the ledger fee).

## Base control-plane transaction affordability

- Use the ETH balance of the Governance Operator, Runtime Administrator, or Independent Canceller selected by the action. There is no separate fixed floor. Do not sign or submit if conservative live Finalized/Safe balances cannot be observed or do not cover the candidate transaction liability.
- Governance fee caps: derive from gas estimates for exact schedule/execute calldata and at least 10 distinct Finalized fee blocks. Set gas limit to 130% of the maximum estimate, rounded up to the next 1,000; max fee to base fee p99×20, priority fee to p95×4, and L1 ceiling to p99×10. Retain 90-second quote validity and the 13,000/60,000/15,000 bps multipliers.

## Settlement cycles reserve

Check the base floor plus a conservative ceiling for each nonterminal liability so new work cannot consume cycles needed to complete existing operations (ADR 0005).

- Settlement cycle ceiling: fixed at `5000000000` cycles. This is not a cap on cycles attached to an external call; it is both the reservation to complete one nonterminal liability and a one-operation margin retained before external calls. Issue paid calls only when the balance covers `required reserve for existing liabilities + actual cycles attached to the call + this margin`.
- Cycles floor: derive from paused `idle_cycles_burned_per_day` using the formula below.

Production install and Gate A use the Bootstrap operating values fixed in the schema 2 template. These are non-operational sentinels, not operating caps; together, the `Bootstrap` lifecycle and shared kernel gate fail closed for asset updates, the scheduler, and Base governance transactions. After deploying Base paused, create `initial-operational-parameters.json`; replace only the eight Governance fee fields, cycles floor, and settlement cycle ceiling in the Gate B profile with derived values, then seal exactly once.
- `cycles floor = (idle cycles burn/day + 5,000,000,000) × 30 × 2`
- `settlement cycle ceiling = 5,000,000,000`
- N: 30 days.

After unpause, Gate C observes at least seven days of Base fees and at least 10 Governance gas and settlement cycles samples each. These observations and `fee-cycles-measurements.json` neither automatically update sealed values nor authorize controller handover. Changes require a separate upgrade and review.

Never insert zero or arbitrary placeholders for unresolved mainnet plan/profile values. Only installation uses the protocol-defined fixed Bootstrap sentinel. Accept only production Canister install plan `schema_version: 2`, install receipt `schema_version: 3`, release profile `schema_version: 5`, Gate A manifest `schema_version: 3`, Gate B manifest `schema_version: 4`, Gate A receipt `schema_version: 2`, and post-Gate-A policy transition `schema_version: 3`. Keep distinct types for the initial controller activation receipt (schema 1), SNS-proposal Activation Receipt for DAO reactivation (schema 5; submission schema 4), and SNS controller handover preparation/completion receipts (schema 5; registration submission schema 1). Old or unknown versions fail closed without migration. Pre-seal `validate-bundle --offline --gate-b` authorizes only sealing; only post-seal `verify-live schedule` authorizes schedule preparation. Do not create or submit transactions when fee caps are exceeded or cycles are insufficient.

## Timelock delay (Base Admin)

- Initial value: 24 hours (ADR 0016).
- Any reduction goes through the Timelock itself.

## External assumption audit checklist

The Bridge cannot guarantee the following internally; operational audits maintain their validity (ADRs 0005 and 0011).

- Upper-bound estimates for gas prices.
- The Canister signs Base governance transactions; the external relayer submits, waits for Finalized status, and notifies confirmation. No automatic resubmission or replacement occurs. Only on an explicit operator request may the Canister re-sign the same nonce/payload up to three times, increasing fees at least 12.5% over the preceding generation without exceeding configured ceilings. Before each signature, checked-compute `gas_limit × max_fee_per_gas + l1_fee_per_transaction_ceiling_wei + value`; if the smaller Safe/Finalized balance is insufficient, reject without changing state.
- Upper-bound estimates for EVM RPC and management canister call costs.
- Settlement retries for transient failures do not share the Governance timer. Use exponential backoff from `settlement_retry_interval_seconds` (initially 60 seconds), capped at 15 minutes. Automatic Deposit and fee payout lanes stop after three consecutive transient failures including the first attempt. Production runs at minutes 0, 1, and 3, waiting for explicit continuation after about three minutes. Progress or explicit deferral resets consecutive failures to zero; `Busy` leaves the count unchanged.
- The official EVM RPC Canister and configured quorum correctly return the canonical Finalized chain.
- Monitoring can demonstrate detection within five minutes, operator acknowledgement within 15 minutes, and pause of both Base and IC within 60 minutes.

The operators, infrastructure, and availability of providers behind the EVM RPC Canister are outside the audit scope and are not production approval conditions.

## Review procedure

1. Do not change mint limits or window duration on an existing contract.
2. If different values are needed, safety-review deployment of a new contract as a separate plan.
3. To change the Service Fee, pause both directions, update the reviewed profile including its relationship to the Ledger fee, and rerun production preflight. Do not independently change Ledger fees and Service Fees during operation.

## Immutable service fee floor

`MIN_SERVICE_FEE` is nonzero and immutable. Construction and updates require `MIN_SERVICE_FEE <= serviceFee <= MAX_SERVICE_FEE`. The approved production minimum is 100_000 raw, matching the fixed Ledger fee; verify the approved deployment profile when preparing artifacts.
