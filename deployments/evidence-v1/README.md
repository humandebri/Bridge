# Evidence bundle v1

Gate A contains exactly these manifest-bound artifacts:

- `profile.json`
- `monitor-drill.json`
- `bridge-canister.wasm`
- `bridge-runtime.bin`

Gate Aの`profile.json`は、まだ存在しないreceipt blockを自己申告せず`deployment_block: 0`とする。
paused deployment後、release commandが実receiptから`gate-a-receipt.json`と`<receipt>.post-deploy-profile.json`を同時生成する。
後者だけをGate Bの`profile.json`として使用し、receiptの`post_deploy_profile_sha256`と実Bridge deployment blockを一致させる。Gate B validatorはprofileを複製して`deployment_block`だけを0へ戻したRFC 8785 canonical hashを`gate_a_profile_sha256`と照合するため、その他のfield driftは署名し直しても拒否される。
Gate B uses a new manifest containing those four files plus `signer-snapshot.json`, `rpc-e2e.json`, `gate-a-receipt.json`, `controller-handover.json`, `sns-upgrade.json`, and `x402-e2e.json`. The receipt binds the Gate A manifest hash, release/source identity, Gate A profile hash, post-deployment profile hash, and both code hashes. Gate B also sets `parent_gate_a_manifest_sha256` to that Gate A hash.

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
  ]
}
```

The actual Gate A manifest must list all four Gate A artifacts exactly once; Gate B must list all ten Gate B artifacts exactly once. Paths must be single relative file paths; symlinks or path traversal outside the bundle are rejected. Gate A omits the parent hash; Gate B requires it. Manifest release-approver signatures and key ceremony artifacts do not exist.

`signer-snapshot.json` records the already-observed live values: `observed_at_unix`, Bridge Canister ID, `chain_id`, official `evm_rpc_canister_id`, Finalized head number/hash, `canonical`, provider quorum counts, Base/Canister signer, deployed and expected runtime bytecode hashes, deployed Wasm hash, the Timelock Keccak runtime code hash, canonical Bridge/Timelock deployment transaction and block bindings, the exact Timelock admin/proposer/executor/canceller membership reconstructed from `RoleGranted`/`RoleRevoked`, actual/expected IC controller, and reserve sufficiency. The observation must be no older than five minutes when Gate B runs. It is evidence input, not a network request made by the CLI. The Mint Signer must match exactly across the approved profile, Canister public configuration, and Finalized Base Bridge state.

`monitor-drill.json` binds the single emergency pause principal to an actual test-canister request ID and audit sequence/digest.

`rpc-e2e.json` is the complete manifest produced and verified by `scripts/evm-rpc-rehearsal/rehearsal.py`; a boolean summary is not accepted. The reviewed verifier source is embedded in the `bridge-profile` binary, so working-directory script replacement cannot alter Gate B. Gate B requires all ten scenarios, raw command artifacts, Canister `EvmRpcObservation` bindings (Canister ID, production call method, internal request/quorum digests, Finalized hash and transaction hash), a fixed module-hash capture, transaction and Ledger references, canonical Finalized confirmation, quorum-loss fail-closed, NonceTooLow handling, and final pause. Provider別全responseとexact agreeing countはEVM RPC client APIの保証境界外なので自己申告せず、configured count、required threshold、故障注入artifact、継続/fail-closed decisionをthreshold certificateとして検証する。Its source revision/tree, RPC URL digests, and Bridge Wasm/runtime hashes must match the release bundle. `monitor-drill.json` records the approved routing hash, one fault origin and the detect, human acknowledgement, Base pause and IC pause timestamps plus public pause references. Every observation/capture must predate the release manifest and be no older than 90 days.

`controller-handover.json` is written only after the fixed ICP CLI command atomically removes every controller and adds the KINIC SNS Root. It binds the request ID, exact argv, exit code, stdout/stderrのraw bytes、combined response digest, executing principal, cycles balance, freezing requirement, and the Root-only final controller set. Validatorはraw transcriptからresponse digestとrequest IDを再導出し、自由記述の成功要約だけを証跡にしない。`sns-upgrade.json` records an Executed KINIC SNS proposal that upgrades the Bridge with the release Wasm and preserves the reviewed public state digest. `x402-e2e.json` records a Base Sepolia EIP-3009 verify/settle through the adopted external facilitator, including the SDK version, facilitator URL digest, matching bSNS runtime hash, and canonical Finalized receipt. Self-operated facilitator or Permit2 evidence is not accepted.

Do not put a seed, private key, backup, device serial, API token, credential-bearing URL, or other secret in this directory.
