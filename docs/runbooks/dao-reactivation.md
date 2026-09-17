# Demonstrate DAO reactivation while retaining the individual controller

This procedure verifies post-launch reactivation. It does not automatically perform initial activation or controller handover. The individual retains code-upgrade and current emergency-pause authority without gaining ordinary administration permissions.

The approved production baseline is schema 36, module SHA-256 `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`, fixed in the [production checkpoint](../../deployments/checkpoints/README.md). This checkpoint was rotated after verifying signed upgrade receipts from historical terminal `6192841b...` through `d48d4737...` to `bf047794...`; saved Wasm and reproducible builds from source revision `16cd903af92878ffe13294cf1dc577550ba1340c` match the live module.

## Starting state

Reverification on 2026-09-16 found Bridge `lb5i5-ziaaa-aaaar-qcgwq-cai` at schema 36, module SHA-256 `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`, with only production identity `lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe` as controller. Governance is `74ncn-fqaaa-aaaaq-aaasa-cai`, pause principal is the production identity, and the Bridge is not registered in SNS Root `dapps`. Retain the initial [baseline manifest](../evidence/dao-baseline-20260914/manifest.json) as a pre-rotation read-only observation.

Retrieve live state again immediately before controller handover, requiring an exact match with active checkpoint `bf047794...` or a subsequently approved current-source suffix. Do not replace a mismatched baseline with raw observations alone. Current `bf047794...` exposes `storage_integrity_check` but not `get_release_storage_integrity`, required for handover completion. Before this runbook's `prepare`, separately validate, approve, and execute a current-source v36 upgrade containing that method and append its receipt to the active checkpoint evidence suffix.

## Preparation

1. Complete applicable proofs and tests on current clean source; fix the Wasm, source revision, and public Candid under review. Execute production upgrades only after separate approval.
2. Query Governance `list_nervous_system_functions`, saving the `icp --json` response including `response_bytes`. Also save Root `list_sns_canisters (record {})`, Bridge management status, runtime, lifecycle, activation, and operational configuration.
3. Run `tools/sns-proposal/prepare.mjs REGISTRY_JSON BRIDGE PREVIOUS_OPERATION OUTPUT_JSON` with pinned Node. PREVIOUS_OPERATION is the latest confirmed operation ID from `get_activation_status`. Do not submit output. Generate registration proposals avoiding active/reserved IDs; existing registrations must exactly match target and validator.
4. Review function IDs, targets, validators, methods, topics, payloads, submitting neuron/signer, and full proposal text for both operations. If registration proposals are needed, obtain separate submission approval and verify executed status and live registry. Registration alone does not reactivate the Bridge.

Both operation payloads from `prepare.mjs` use the operation ID from the same observation. Regenerate with the new ID after schedule confirmation; do not submit the previously prepared execute payload.

Dedicated APIs are `validate_sns_schedule_activation`/`sns_schedule_activation` and `validate_sns_execute_activation`/`sns_execute_activation`, each taking `record { previous_governance_operation_id : nat64 }`. Validators are read-only updates returning `Result<text,text>`; execution entry points return failures as IC rejects. SNS executed status means signature preparation completed, not Base execution.

## Production demonstration (after separate approval for each action)

- Specify pause time and small test amounts in execution proposals/work records. Pause using the current emergency principal and verify both Base flows and IC Deposits are stopped.
- Prepare and submit schedule proposals from the latest registry and confirmed operation. Append PREVIOUS_OPERATION and the reviewed proposal-preparation JSON path to existing `production-activation-proposal.sh`. Regenerate from the latest registry before submission and verify function ID, validator, payload, and proposal text match. Never resubmit a checkpointed submission; reread the CLI response saved in `OUTPUT.response.json` and proposal history. Do not delete journals even if response decoding fails.
- Relay the signed transaction, complete notification through the designated confirmation relayer, and finish Canister Finalized verification. Stop on revert; do not classify it as a confirmation timeout.
- After 24 hours, retrieve live state/registry again and separately prepare, approve, and submit an execute proposal bound to the confirmed schedule operation ID. Complete relay and Finalized verification, checking resumed Base flows/IC Deposits, small deposits/withdrawals, and continuity of data, reserves, and audits.
- SNS submissions use schema 4 and activation receipts schema 5. Do not convert old SNS formats to pass. Retain initial controller receipts as a distinct type.
- Fee and recipient changes are outside this production demonstration.

For reactivation receipt commands `bridge-profile verify-activation` and `verify-schedule-receipt-live`, specify the approved current v36 terminal through `BRIDGE_CHECKPOINT_EVIDENCE`. Verify initial Gate B as history and derive only the current module from the checkpoint. Gate B sole-controller requirements remain unchanged.

## Final handover

Do not add Root, register the dapp, or remove the individual controller before operators review the demonstration and separately approve handover. Even after approval, do not replace controllers with Root alone in one step. Handover-driver `prepare` only adds Root, verifies exactly the production identity and Root as joint controllers, and saves a schema 5 preparation receipt. On unknown outcome, retrieve state with `recover`; do not resubmit administration calls.

After verifying joint control, submit the reviewed standard `RegisterDappCanisters` proposal exactly once through `production-handover-registration-proposal.sh`. Submission/execution failure retains the individual controller without automatic resubmission or removal. After verifying proposal executed status, driver `complete` checks the Governance proposal, Root `dapps`, Root-only control, and module/runtime/storage continuity, saving a schema 5 completion receipt separately. Stop as an incident if Root-only but unregistered, registered but still individually controlled, or any third controller exists.

Pass seal/schedule/execute receipts through environment variables to the registration submission script too. Before submission it reruns the existing typed handover validator and proof gate, reserving the journal and submitting only if reviewed payload exactly matches the fixed single-Bridge action.

To abort testflight, recheck exactly two controllers—production identity and Root—and that registration has not executed, then remove Root through a separately approved action. After completion, use UpgradeSnsControlledCanister with the same uncompressed Wasm, mode=3 (upgrade), and empty Candid arguments; verify proposal success, preserved state, runtime, deposits/withdrawals, and UI.

Generate handover submissions as follows without sending them. ROOT_RESPONSE_JSON is Root's latest `--json` response to `list_sns_canisters (record {})`; EXPECTED_SHA256 is the verified current uncompressed module hash.

```sh
node tools/sns-proposal/handover.mjs ROOT_RESPONSE_JSON BRIDGE WASM EXPECTED_SHA256 OUTPUT_JSON
```

Output includes registration status, registration proposal, standard same-Wasm upgrade proposal, one-megabyte chunks, and each SHA-256. If the Bridge is already registered with Root when handover begins, stop on inconsistent state; do not skip registration and continue. Since Bridge Wasm exceeds ingress limits, separately approved handover preparation lets the individual controller upload to the Bridge's own chunk store, then reread returned hashes and `stored_chunks`. Finish uploads before adding Root. The proposal fixes ordered chunk hashes and original uncompressed Wasm hash. Do not change module hash by compression. Chunked upgrades also use the standard SNS path. [Official SNS management procedure](https://docs.internetcomputer.org/guides/governance/managing/)

Retain the emergency pause principal. Do not assume the individual identity can directly repair post-handover failures.

## Local validation

`scripts/prepare-sns-test-runtime.sh` extracts Governance/Root from the hash-pinned official `release-2026-09-10_03-28--all-in-one-node` archive. Set `BRIDGE_SNS_TEST_RUNTIME` to choose the destination. Start Root with testflight=false.

`real SNS reactivation and production registration` in `integration/phase3.spec.ts` verifies real Governance registration/adoption/execution, Bridge reactivation, retention of the individual controller, its removal through Root registration, and same-Wasm upgrade. Base RPC and Ledger remain mocked, so this does not replace the production 24-hour wait or real-asset checks.

Local early execute may succeed through signature preparation. Base Timelock enforces 24 hours, so SNS executed status does not imply reactivation. Real SNS tests notify mocked reverts and verify continued pause; existing Solidity tests verify Timelock timing itself.

With a checkpoint, the handover driver matches historical initial Gate B/seal/controller activation to checkpoint roots, then requires current clean source to match terminal source, complete proofs, and identical module hashes from two rebuilds. Verify current individual-only control separately from post-handover Root-only control.

Root performs the actual upgrade after responding to the request, so proposal executed status and unchanged module hash alone do not prove completion. Use `get_release_upgrade_observation` to verify caller and completion time of a successful SNS Root `post_upgrade` after adoption/handover. This record is heap-only and changes no stable schema; failed upgrades do not update it. Assume operationally that no other upgrade runs concurrently; if another proposal upgrades during validation, reacquire evidence. Local real SNS tests distinguish a post_upgrade trap after Root response from a subsequent successful same-Wasm upgrade.
