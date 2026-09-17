---
status: accepted
---

# Keep the Bridge canister upgradeable and hand it over to SNS control

Keep the Bridge canister upgradeable. Retain the production identity as sole controller during the DAO operation demonstration after public launch. After demonstrating reactivation through a DAO proposal, operators decide handover timing separately without a fixed observation period. Confirmation of the initial execute permanently consumes only the internal bootstrap activation authority; it does not determine when external controller settings change. If operators separately approve handover, make SNS Root the sole controller, after which only adopted SNS Governance proposals approve upgrades.

## Staged handover

1. Retain the individual sole controller and current emergency pause principal while demonstrating schedule/execute reactivation through SNS custom proposals. Do not include production changes to fees or recipients in this demonstration.
2. After explicit approval, add SNS Root as a controller and verify joint control by the individual identity and Root. Then execute the standard `RegisterDappCanisters` proposal, verifying Root registration and transition to Root as sole controller. Do not begin this procedure from an already-registered state.
3. Upgrade to the same Wasm through a standard `UpgradeSnsControlledCanister` proposal and verify preserved state and continued asset admission to complete handover. Do not remove the emergency pause principal.

Production SNS Root registration removes controllers other than Root. Do not apply testflight behavior that retains joint controllers to production. Do not use a path that demonstrates standard SNS upgrades while retaining the individual controller. Do not assume direct individual repair is available if registration or upgrade fails after handover.

## Considered Options

- Reject removing all controllers to make the Bridge canister immutable because it would prevent responses to stable-state failures, IC API changes, and dependency updates.
- Reject retaining the developer identity as a co-controller after handover because it would permit upgrades without SNS proposals.
- Reject setting SNS Governance directly as controller because the standard SNS architecture uses Root as the application canister controller executing upgrades.
- Adopt SNS Root as sole controller, with upgrade authority delegated to SNS Governance proposals.

## Consequences

- The production identity being a controller does not itself prohibit production asset admission after initial activation. Admission depends on Gate B, Confirmed execute, pause state, and operating limits.
- Do not use the seven-day, 10-per-type production measurements after unpause or `fee-cycles-measurements.json` as handover authorization inputs. Bind handover to initial operating values, seal/schedule/execute receipts, live RuntimeBinding, and the current profile Wasm, with separate explicit approval.
- Immediately before adding Root, require the production identity as sole controller, Activated state, and unpaused Base Deposits, Base Withdrawals, and IC Deposits. After adding Root, require exactly the production identity and SNS Root; submit the registration proposal only during this joint-controller period. Completion requires proposal execution, unique Root registration, and SNS Root as the only controller, retaining no developer identity, fallback identity, or NNS Root.
- Do not equate the initial install hash with the live module. Verify the chain from the post-Gate-A policy transition and normal upgrade receipts to the current profile Wasm.
- Save the module, RuntimeBinding, storage integrity, activation/pause state, and record/audit counts before and after controller changes as raw evidence and verify continuity. Do not require clearing live state.
- After handover, attach the Wasm hash, source revision, Verus results, test results, and stable schema compatibility to each SNS upgrade proposal.
- Persist Rust state directly in stable structures; avoid serializing all state in `pre_upgrade`.
- Verify that unfinished Deposits, Withdrawals, EVM transactions, and Reconciliation Holds can resume across upgrades.
- Do not make the Runtime Administrator a canister controller. Separate pause, Service Fee, and Fee Recipient permissions from upgrade authority.
- SNS Governance can change Bridge logic through upgrades and is therefore the ultimate trust authority for ICP-side code. This authority cannot change immutable Base contract constraints.
- Rerun Verus proofs for each Wasm; do not reuse proofs from a previous version for a new upgrade.
