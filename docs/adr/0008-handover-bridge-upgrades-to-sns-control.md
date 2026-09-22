---
status: accepted
---

# Keep the Bridge canister upgradeable and hand it over to SNS control

Keep the Bridge canister upgradeable. Before demonstrating DAO operations, validate the certified current state and reproducible current-source Wasm, add SNS Root, and prove exact joint control by the production identity and Root. After demonstrating reactivation through DAO proposals under joint control, stop without automatically registering the dapp or removing the production identity; operators decide full-handover timing separately. Confirmation of the initial execute permanently consumes only the internal bootstrap activation authority and does not determine when external controller settings change. If operators separately approve full handover, make SNS Root the sole controller, after which only adopted SNS Governance proposals approve upgrades.

## Staged handover

1. Verify the certified individual sole controller, current module, runtime, storage, lifecycle, and clean-source reproducibility. After explicit approval, add only SNS Root and verify joint control by the individual identity and Root.
2. While retaining joint control, demonstrate schedule/execute reactivation through SNS custom proposals. Do not include production changes to fees or recipients, and stop with joint control after the demonstration.
3. Only after separate explicit approval, execute the standard `RegisterDappCanisters` proposal and verify Root registration and transition to Root as sole controller. Do not begin this procedure from an already-registered state. Do not persist a handover receipt; the authenticated current state and Governance proposal are authoritative.
4. After a second explicit approval, upgrade to the same Wasm through a standard `UpgradeSnsControlledCanister` proposal and verify the SNS Root post-upgrade observation, preserved state, and continued asset admission to complete handover. Standard SNS Governance rejects this upgrade before Root registration, even when Root is already a co-controller. Do not remove the emergency pause principal.

Production SNS Root registration removes controllers other than Root. Do not apply testflight behavior that retains joint controllers to production. Do not use a path that demonstrates standard SNS upgrades while retaining the individual controller. Do not assume direct individual repair is available if registration or upgrade fails after handover.

## Considered Options

- Reject removing all controllers to make the Bridge canister immutable because it would prevent responses to stable-state failures, IC API changes, and dependency updates.
- Reject retaining the developer identity as a co-controller after handover because it would permit upgrades without SNS proposals.
- Reject setting SNS Governance directly as controller because the standard SNS architecture uses Root as the application canister controller executing upgrades.
- Adopt SNS Root as sole controller, with upgrade authority delegated to SNS Governance proposals.

## Consequences

- The production identity being a controller does not itself prohibit production asset admission after initial activation. Admission depends on Gate B, Confirmed execute, pause state, and operating limits.
- Do not use the seven-day, 10-per-type production measurements, historical upgrade records, or initial activation receipts as handover authorization inputs. Bind handover to certified current state, complete current proofs, and the twice-reproduced current-source Wasm, with separate explicit approval.
- Immediately before adding Root, require the production identity as sole controller and an exact certified match of module/runtime/storage/lifecycle state. After adding Root, require exactly the production identity and SNS Root and demonstrate DAO reactivation in that state. Submit the registration proposal only after separate full-handover approval. Completion requires proposal execution, unique Root registration, and SNS Root as the only controller, retaining no developer identity, fallback identity, or NNS Root.
- Do not equate the initial install hash or any historical receipt with the live module. Verify the certified current module directly.
- Compare the module, RuntimeBinding, operational configuration, storage integrity, activation/pause state, history indexes, balances, epochs, and record/audit counts in memory before and after adding Root. Permit only the expected controller-set change and write no operation evidence file.
- After handover, attach the Wasm hash, source revision, Verus results, test results, and stable schema compatibility to each SNS upgrade proposal.
- Persist Rust state directly in stable structures; avoid serializing all state in `pre_upgrade`.
- Verify that unfinished Deposits, Withdrawals, EVM transactions, and Reconciliation Holds can resume across upgrades.
- Do not make the Runtime Administrator a canister controller. Separate pause, Service Fee, and Fee Recipient permissions from upgrade authority.
- SNS Governance can change Bridge logic through upgrades and is therefore the ultimate trust authority for ICP-side code. This authority cannot change immutable Base contract constraints.
- Rerun Verus proofs for each Wasm; do not reuse proofs from a previous version for a new upgrade.
