# Evidence bundle v1

Gate A contains exactly these manifest-bound artifacts:

- `profile.json`
- `ceremony.json`
- `monitor-drill.json`
- `bridge-canister.wasm`
- `bridge-runtime.bin`

After paused deployment, Gate B uses a new manifest containing those five files plus `signer-snapshot.json`, `rpc-e2e.json`, and `gate-a-receipt.json`. The receipt binds the Gate A manifest hash, release/source identity, profile hash, and both code hashes. Gate B also sets `parent_gate_a_manifest_sha256` to that Gate A hash.

`release-manifest.json` has the following shape. Hashes are lowercase or uppercase 64-digit SHA-256 values. Timestamps are Unix seconds and the validity window must not exceed 90 days.

```json
{
  "schema_version": 1,
  "release_id": "release-identifier",
  "test_only": false,
  "source_revision": "reviewed-revision",
  "source_tree_sha256": "64-hex-digits",
  "created_at_unix": 0,
  "expires_at_unix": 0,
  "parent_gate_a_manifest_sha256": null,
  "artifacts": [
    { "path": "profile.json", "sha256": "64-hex-digits" }
  ],
  "approval": {
    "signer": "0x-release-approver",
    "eip191_signature": "0x-65-byte-signature"
  }
}
```

The actual Gate A manifest must list all five Gate A artifacts exactly once; Gate B must list all eight Gate B artifacts exactly once. Paths must be single relative file paths; symlinks or path traversal outside the bundle are rejected. The signature is over `SHA-256(JCS(manifest without approval))` as a 32-byte EIP-191 personal message. Gate A omits `approval` and the parent hash; Gate B requires both.

`signer-snapshot.json` records the already-observed live values: `observed_at_unix`, Bridge Canister ID, `chain_id`, official `evm_rpc_canister_id`, Safe-confirmed head number/hash, `canonical`, provider quorum counts, Base/Canister signer, release-bound chain-key EIP-191 signature, deployed and expected runtime bytecode hashes, deployed Wasm hash, Timelock address/delay/self-admin, actual/expected IC controller, and reserve sufficiency. The observation must be no older than five minutes when Gate B runs. It is evidence input, not a network request made by the CLI.

Create the chain-key signature through the Bridge Canister update endpoint `sign_chain_key_challenge(release_id)`. The endpoint accepts only `[a-z0-9-]{8,64}`, permits only the configured governance principal, and signs the fixed SHA-256 challenge documented by the validator as a 32-byte EIP-191 personal message. `scripts/production-live-preflight.sh capture` calls this endpoint with `BRIDGE_DFX_IDENTITY` and never copies a signature from an older snapshot.

`ceremony.json` contains only public role addresses, the backup-restore result, and exactly one address-control challenge for each of `release_approver`, `base_admin`, `timelock_canceller`, and `runtime_administrator`. Each challenge records a non-secret custodian identifier, device class, device failure domain, and EIP-191 signature over the release/role/address-bound challenge. The canceller custodian and device failure domain must differ from every other hardware role. The Canister chain-key signer is verified after deployment by a separate release-bound signature in `signer-snapshot.json`. `contains_secret_material` must be false.

`rpc-e2e.json` is the complete manifest produced and verified by `scripts/evm-rpc-rehearsal/rehearsal.py`; a boolean summary is not accepted. The reviewed verifier source is embedded in the `bridge-profile` binary, so working-directory script replacement cannot alter Gate B. Gate B requires all ten scenarios, raw command artifacts, transaction and Ledger references, canonical Safe confirmation, quorum-loss fail-closed, NonceTooLow handling, and final pause. Its source revision/tree, RPC URL digests, and Bridge Wasm/runtime hashes must match the release bundle. `monitor-drill.json` records the approved routing hash, one fault origin and the detect, human acknowledgement, Base pause and IC pause timestamps plus public pause references. Every observation/capture must predate the release manifest and be no older than 90 days.

Do not put a seed, private key, backup, device serial, API token, credential-bearing URL, or other secret in this directory.
