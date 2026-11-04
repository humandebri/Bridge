# SNS Root-only recovery and upgrades

## Current scope and historical record

On October 1, 2026, the certified controller observation recorded SNS Root as the sole Bridge controller. It is historical evidence, not authorization for a future operation; see the [production status](../../README.md#production-status) and [historical baseline](../evidence/dao-baseline-20260914/manifest.json). The former joint-control reactivation and one-time Root addition drivers have been retired. Do not re-add an individual controller or repeat registration to recover the current production Bridge. Emergency pause authority is separate from canister controller authority.

## Root-only recovery

The retired joint-control path and its locally asserted controller receipt must not be used for recovery. The Root-only verifier binds certified current controller/module information, signed v36 state, the executed SNS proposal and registered function, and the exact pending signed Base transaction. It checks that the current state is unchanged when compared with the separately reviewed evidence. It does not prove that state cannot change temporarily and return to its earlier value between sequential observations.

1. Preserve the incident evidence, including the certified module and controller set, signed query results, SNS proposal and execution status, and any Base transaction receipt. Check a receipt for a revert before waiting for finality. Do not automatically resubmit an uncertain action.
2. Reacquire certified controller/module information and signature-verified individual v36 lifecycle, activation, RuntimeBinding, operational configuration, storage integrity and complete history-index observations. Check SNS Root registration and the reviewed UI RPC configuration. Stop if evidence is unavailable, inconsistent, or changes between review and the immediate pre-submission observation.
3. Determine whether the issue requires an SNS governance action, a separately reviewed Root-controlled canister upgrade, or investigation without a state change. Do not assume that a former individual controller can run controller-only methods. Do not assume an upgrade repairs corrupt storage. Rebuilding history indexes resumes from the saved cursor after a reviewed same-instance upgrade, but an error needs investigation first.
4. Before any action, prepare and review its exact proposal and evidence, confirm the current module immediately before submission, and obtain the separately required operational approval. After execution, verify the actual effects and preserved state. SNS proposal execution or a successful call alone does not prove a Base transaction finalized or that a canister upgrade completed.

For an already executed SNS activation proposal with a pending Base transaction, retain its submission JSON and save the exact pending transaction from the governance relayer's `status` command as `ARTIFACT_JSON`. Set `BRIDGE_CURRENT_MODULE_SHA256` to the reviewed certified module hash, and capture the authenticated pending-state evidence:

```sh
cargo run --locked -p bridge-profile -- verify-root-activation-pending PHASE BUNDLE SUBMISSION_JSON ARTIFACT_JSON capture EVIDENCE_JSON
```

Review the proposal, signed transaction and newly created evidence before any separately authorized action. The same command with `compare` in place of `capture` reacquires the evidence and rejects any difference. The existing attestation in the Canister is validated as a configuration binding; it is not claimed to be a fresh observation of Base state. The Canister performs its own Base preflight before preparing the pending activation transaction.

For a separately approved relay or confirmation, `scripts/production-root-reactivation-driver.sh` freezes the inputs, reruns the complete proof gate from the clean current HEAD, compares the authenticated pending state immediately before the operation, and then invokes the governance relayer. Set `BRIDGE_RELEASE_BUNDLE`, `BRIDGE_GATE_B_MANIFEST_SHA256`, `BRIDGE_CURRENT_MODULE_SHA256`, `BRIDGE_ACTIVATION_PHASE`, `BRIDGE_ACTIVATION_SUBMISSION`, `BRIDGE_ACTIVATION_ARTIFACT`, `BRIDGE_ACTIVATION_EVIDENCE`, `BRIDGE_ACTIVATION_STEP` (`relay` or `confirm`) and the phase-specific `BRIDGE_CONFIRM_ASSET_ACCEPTANCE`. The relayer also requires the reviewed `BASE_RPC_URL`. Confirmation additionally requires `BRIDGE_CONFIRMATION_RELAYER_PEM`, new paths for `BRIDGE_ACTIVATION_CONFIRMATION_RECEIPT` and `BRIDGE_ACTIVATION_VERIFIED_RECEIPT`, and, for execute, `BRIDGE_PRIOR_SCHEDULE_RECEIPT`. The driver verifies the SNS result after confirmation. If confirmation succeeds but its final verification fails, retain the confirmation receipt and investigate rather than retrying automatically.

These commands do not create or submit SNS proposals. Those actions and any Base relay or confirmation require separate operational decisions. In particular, do not use the old `verify-activation` or `verify-schedule-receipt-live` commands for Root-only control; the corresponding Root-only post-confirmation commands are `verify-root-activation` and `verify-root-schedule-receipt-live`.

Individual authenticated queries are sequential: matching observations before and after validation detect observed differences, but cannot prove that state did not change temporarily and return to its earlier value between queries. Treat concurrent changes as an incident and repeat the observation after they stop.

## Preparing a different Wasm under SNS control

This is a preparation and review procedure. It does not create a canister, upload chunks, submit a proposal, or authorize a production upgrade. The existing `production-handover-registration-proposal.sh` upgrade path is restricted to the already stored **same Wasm**; it cannot prepare or validate this different-Wasm procedure.

1. From the clean current HEAD, complete the proof gate and reproduce the candidate uncompressed Wasm twice. Record the source fingerprint, complete receipt, both candidate hashes, and the certified current Bridge module hash. Review the upgrade mode, arguments, schema-v36 compatibility and state preservation requirements. Reject a candidate that differs from either reproduction.
2. Select a dedicated chunk-store canister on the **same subnet** as the Bridge. Verify its subnet using authenticated network evidence and its exact certified controllers. SNS Root must control both the Bridge and the store so it can install from that store; the designated uploader must control the store to upload and inspect its chunks. Record the principals and permissions. Creating or changing a store and funding it are separate actions, outside this procedure.
3. Divide the exact reviewed uncompressed Wasm into the management API's permitted chunks, record every SHA-256 in installation order and the whole-module SHA-256. After separately authorized upload, read `stored_chunks` and verify that every required hash is present; the returned set does not establish ordering, so independently compare the proposal's ordered list against the locally reconstructed candidate. Avoid clearing a shared store without reviewing its other users.
4. Prepare the precise SNS upgrade proposal with Bridge target, dedicated store principal, upgrade mode and arguments, ordered chunk hashes, and candidate module hash. The following local-only commands prepare a review artifact and compare its complete proposal, ordered chunk list and candidate against a `stored_chunks` response envelope. They do not upload chunks or call SNS:

   ```sh
   node tools/sns-proposal/handover.mjs root-upgrade-prepare BRIDGE STORE WASM CURRENT_SHA256 CANDIDATE_SHA256 > PREPARED_JSON
   node tools/sns-proposal/handover.mjs root-upgrade-verify-chunks PREPARED_JSON BRIDGE STORE WASM CURRENT_SHA256 CANDIDATE_SHA256 STORED_CHUNKS_JSON
   ```

   The local helper does not authenticate its supplied stored-chunk envelope. After a separately authorized upload, use the uploader's identity to obtain and review authenticated evidence:

   ```sh
   cargo run --locked -p bridge-profile -- verify-sns-root-upgrade-preflight PROFILE CURRENT_SHA256 CANDIDATE_SHA256 WASM STORE UPLOADER UPLOADER_PEM REGISTRATION_PROPOSAL_ID PREPARED_JSON capture EVIDENCE_JSON
   ```

   The command verifies the exact locally reconstructed proposal, certified Root-only Bridge control and current module, signed v36 state, executed registration proposal, matching delegated subnet certificates, the Root/uploader permissions, and the candidate chunks returned by the management canister. It records the complete controller set for review and comparison, including any additional controller who can change the store. It captures the observed values and hashes into a new evidence file. Listing stored chunks is an authenticated management-canister update; this command does not upload, install, or submit a proposal. The uploader PEM must correspond to the designated store controller.
5. Review and approve the exact prepared proposal and evidence. Immediately before any separately authorized submission, repeat the preceding command with `compare` in place of `capture` and the same evidence file. It reacquires the observations and rejects any difference. Reproduced Wasm and the complete current-source proof gate remain separate mandatory checks; the preflight does not establish those results or submit a proposal. Prevent concurrent upgrades and store modifications throughout the action.
6. After execution, use the read-only verifier to check the authenticated registration and upgrade proposals, candidate Wasm and ordered chunks, certified installed candidate and Root-only control, and successful Root `post_upgrade` observation. It checks v36 state and repeats the production state observation around proposal and hook verification:

   ```sh
   cargo run --locked -p bridge-profile -- verify-sns-root-upgrade-live PROFILE CANDIDATE_SHA256 WASM REGISTRATION_PROPOSAL_ID UPGRADE_PROPOSAL_ID STORE
   ```

   This post-execution check does not establish the pre-submission current module, store permissions, subnet or upload evidence; preserve and verify those independently as described above. Confirm the reviewed UI RPC profile and reassess UI publication separately. Proposal execution alone is insufficient.

The [IC management API specification](https://docs.internetcomputer.org/references/ic-interface-spec/management-canister/) defines controller, same-subnet and chunk-hash requirements. External assumptions include the authenticity of certified IC state and signed query responses, SNS/management-canister semantics, and the absence of concurrent upgrades during the observation interval.

## Local evidence

The local real-SNS integration exercises the historical joint-control handover and same-Wasm scenario with mocked Base RPC and Ledger; it does not establish production recovery, a different-Wasm upgrade, real-asset behavior, or the production Timelock wait. Preserve historical test and receipt records as evidence of what was actually observed.
