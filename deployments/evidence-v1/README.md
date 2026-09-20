# Evidence bundle v2

Gate A contains exactly these manifest-bound artifacts:

- `profile.json`
- `bridge-canister.wasm`
- `bridge-runtime.bin`
- `bsns-creation.bin`
- `bsns-runtime.bin`
- `bsns-runtime-layout.json`

Gate A `profile.json` uses `deployment_block: 0`, without self-reporting a receipt block that does not yet exist.
After paused deployment, the release command generates `gate-a-receipt.json` and `<receipt>.post-deploy-profile.json` together from actual receipts.
Gate B `profile.json` starts from the latter, replacing only eight Governance EVM fee fields, `cycles_floor`, and `settlement_cycle_ceiling` with derived values from `initial-operational-parameters.json`. The Gate B validator normalizes these 10 fields to Bootstrap values and checks `post_deploy_profile_sha256`, then resets `deployment_block` to 0 and compares the RFC 8785 canonical hash with `gate_a_profile_sha256`. Other field drift is rejected even if the manifest is recreated.
Gate B uses a schema 4 manifest containing exactly the six current release build files plus `initial-operational-parameters.json`, `provider-independence.json`, `ui-assets.json`, `gate-a-receipt.json`, and `gate-a-profile.json`. Initial evidence binds exact schedule/execute calldata gas estimates, at least ten distinct Finalized fee blocks, and the live idle cycles burn observation. The validator re-derives the fixed fee/cycles ceilings and requires an exact profile match. Gate A activation records remain retained, but historical upgrade receipts and checkpoint chains are not release authority. Current production authorization instead requires a clean source revision, current proofs, two identical Wasm builds, and authenticated live module/controller/runtime/state observations. Ordinary JSON artifacts remain limited to 16 MiB. Gate B sets `parent_gate_a_manifest_sha256` to the Gate A hash.

`provider-independence.json` schema v2 is a deterministic binding receipt, not an SNS Motion receipt or a claim that opaque upstreams are organizationally independent. It binds the release ID/source/tree/profile/current Wasm, official EVM RPC Canister, chain ID 8453, the `BaseMainnet` default provider pool, an empty custom-RPC URL digest, and the production runtime's fixed selection of three providers with a two-response threshold. The release manifest hashes the receipt, while the fresh activation attestation binds the same official Canister and Base runtime state. The EVM RPC default-provider registry and each provider's upstream chain remain an explicit external assumption. Changing the Bridge-side service binding requires a reviewed Wasm upgrade and a fresh activation attestation; an unrelated governance proposal about the empty custom provider list is neither evidence of the built-in pool nor a Gate B prerequisite. Generate the receipt before the manifest with `bridge-profile write-provider-independence-receipt PROFILE RELEASE_ID SOURCE_REVISION SOURCE_TREE_SHA256 OUTPUT`.

`release-manifest.json` has the following shape. Hashes are lowercase or uppercase 64-digit SHA-256 values. Timestamps are Unix seconds and the validity window must not exceed 90 days.

```json
{
  "schema_version": 3,
  "release_id": "release-identifier",
  "test_only": false,
  "source_revision": "reviewed-revision",
  "source_tree_sha256": "64-hex-digits",
  "created_at_unix": 0,
  "expires_at_unix": 0,
  "parent_gate_a_manifest_sha256": null,
  "artifacts": [
    { "path": "profile.json", "sha256": "64-hex-digits" }
  ]
}
```

The actual Gate A manifest must list all six Gate A artifacts exactly once; Gate B must list all eleven Gate B artifacts exactly once. Paths must be single relative file paths; symlinks or path traversal outside the bundle are rejected. Gate A omits the parent hash; Gate B requires it. Manifest release-approver signatures and key ceremony artifacts do not exist.

Gate B reads the public RuntimeBinding and reserve status through signature-verified Canister queries. The schedule/execute prepare wrapper uses the fixed confirmation relayer identity to refresh the attestation immediately before verification; manual `governance-relayer refresh-attestation` is diagnostic only. The authenticated Finalized Base attestation must postdate the Gate B manifest and be no older than five minutes. It binds Bridge, Timelock, BSNS, pause, signer, runtime, role and fee state, including an exact Timelock delay match. Operational configuration is read separately with a controller or governance identity.

`monitor-drill.json` schema v4 binds the emergency pause claim to exact Finalized Base actions and an IC request certificate. It is not a Gate A/Gate B authorization input; as post-unpause Gate C operational evidence, verify response, certificate, audit digest, timestamp ordering, and the exact pause/cancel action set. Use `production-live-preflight.sh verify-monitor-drill` only for direct RPC checks in staging monitor drills; production Base state is authoritative through activation attestation saved via the official EVM RPC Canister. This evidence does not authorize controller handover.

Post-launch Gate C requires `rpc-e2e.json`, `monitor-drill.json`, `monitoring-receipt.json`, and `keeper-drill.json` together with seven-day, at-least-10-per-type production measurements and operation continuity evidence. Exclude them from unpause-authorizing Gate B and controller handover authorization inputs.

`rpc-e2e.json` schema v2 is the manifest produced and verified by `scripts/evm-rpc-rehearsal/rehearsal.py`; a boolean summary is not accepted. The template and standalone verifier remain available for post-unpause Gate C evidence, but the current schema is not yet admissible: it conflates production-release and rehearsal Wasm hashes, the monitor schema conflates production and rehearsal pause principals, outer staging v8 requires all ten scenarios despite the inner launch-ready split, and the fixed `quorum_loss` injector records `request_deposit` while its validator requires `notify_withdrawal`. The reviewed fixed staging URLs also expose no fault-control API. These inconsistencies must be replaced and reviewed before Gate C collection; they must not be bypassed or used to authorize Gate B, activation, or handover. Quorum-loss safety remains mandatory PocketIC/proof negative evidence. Production bundle freeze retains its no-follow/hash/size validation for later evidence handling.

Controller handover runs only after separate approval. It uses the clean current source, complete proofs, two identical Wasm builds, and authenticated live state. Immediately before adding SNS Root, the controller set must be exactly the production identity, the module must match the reproduced Wasm, lifecycle must be Activated, flows must be unpaused, reserves and storage must be healthy, and indexes must be ready. Success is exactly `{production identity, SNS Root}`. The driver writes no handover receipt and never removes the production controller or registers the Dapp.

Activation submission and receipt files deliberately remain outside the fixed eleven-artifact Gate B bundle. Initial activation uses schema 1 controller authorization, preparation, confirmation, and completion receipts binding the sole production installer controller, certified `get_activation_status`, and Canister-independently verified Finalized Base transactions. Do not use schema v3 generic-function submissions or schema v4 SNS proposal activation receipts for initial activation. Post-handover reactivation authorization binds the certified joint-controller set and current module hash. In both receipt paths, execute receipts hash-bind verified schedule receipts.

Do not put a seed, private key, backup, device serial, API token, credential-bearing URL, or other secret in this directory.
