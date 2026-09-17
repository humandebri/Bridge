# Governance relayer CLI

A CLI for operators to explicitly submit and confirm Base governance transactions threshold-signed by the Bridge Canister. It uses no EVM private key.

```bash
export BRIDGE_CANISTER_ID='...'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
export IC_IDENTITY_PEM='/secure/path/governance.pem'
npm run governance-relayer -- prepare --action pause-deposits
unset IC_IDENTITY_PEM
export IC_IDENTITY_PEM='/secure/path/confirmation-relayer.pem'
npm run governance-relayer -- run
```

Commands include `seal-operational-config`, `prepare`, `prepare-schedule-activation`, `prepare-execute-activation`, `status`, `relay`, `confirm`, `run`, `replace`, `refresh-attestation`, and `drain-emergency`. Only the two initial-activation prepare commands use the production controller identity, exclusively creating JSON at `--artifact-file` in an immutable release-evidence directory. `relay` rejects unless that file exactly matches the latest live pending attempt. In addition to exact matches, `confirm` accepts a fixed pre-replacement artifact from the same pending activation lineage; the Canister rechecks against saved attempt hashes before EVM RPC. `relay` can run anonymously; `confirm` uses only the fixed confirmation relayer identity and exclusively writes `--receipt-file`. No command performs activation prepare/relay/confirm consecutively with one identity. Replacement validates the old artifact and saves the new generation to a separate `--output-artifact-file`.

The canonical transaction-hash option for `confirm` is `--transaction-hash`. Accept `--hash` as a short alias, but fail closed on both together, duplicate options, or unknown options not belonging to the command.

`IC_IDENTITY_PEM` is required for `confirm`, `run`, `prepare`, `replace`, activation preparation, `refresh-attestation`, and emergency operations. Initial activation preparation uses the production controller fixed at seal time. After Confirmed execute consumes internal bootstrap activation authority, use only the Governance identity regardless of external controller-change timing. Use the dedicated confirmation relayer identity fixed in the release profile for `confirm`, `run`, and attestation refresh. Service Fee changes use Governance; pause, recorded Timelock cancellation, and `drain-emergency` may use Governance or pause identity. `status` and raw-transaction `relay` remain anonymous. Do not trust confirmation callers' reports: check saved operation ID and signed-generation hash before RPC; the Canister independently observes receipts and state.

`run` and `drain-emergency` stop Finalized polling immediately on a reverted receipt. For activation, first save preparation JSON and Gate B authorization/binding, then use a separate process for `relay --artifact-file <artifact.json> --authorization-file <authorization.json> --binding-file <prepare-receipt.json>`, Finalized confirmation, and dedicated-relayer `confirm --artifact-file <artifact.json> --authorization-file <authorization.json> --binding-file <prepare-receipt.json> --receipt-file <confirmation.json>`. Independent Canister observations finalize the operation. Activation kinds reject unbound `relay`/`confirm` and generic `run`/`replace`; fee replacement uses driver `replace-activation` to bind a new generation to the same authorization.

Standalone `relay` sees only the submission RPC response; neither a returned success hash nor `already known` is final confirmation. The CLI always displays `Unverified`; terminal completion requires independent Finalized observation by the Canister.
