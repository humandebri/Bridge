# Evidence bundle v2

Gate A contains exactly these manifest-bound artifacts:

- `profile.json`
- `monitor-drill.json`
- `bridge-canister.wasm`
- `bridge-runtime.bin`

Gate Aの`profile.json`は、まだ存在しないreceipt blockを自己申告せず`deployment_block: 0`とする。
paused deployment後、release commandが実receiptから`gate-a-receipt.json`と`<receipt>.post-deploy-profile.json`を同時生成する。
後者だけをGate Bの`profile.json`として使用し、receiptの`post_deploy_profile_sha256`と実Bridge deployment blockを一致させる。Gate B validatorはprofileを複製して`deployment_block`だけを0へ戻したRFC 8785 canonical hashを`gate_a_profile_sha256`と照合するため、その他のfield driftは署名し直しても拒否される。
Gate B uses a new manifest containing those four files plus `signer-snapshot.json`, `rpc-e2e.json`, `gate-a-receipt.json`, `controller-handover.json`, and `sns-upgrade.json`. The receipt binds the Gate A manifest hash, release/source identity, Gate A profile hash, post-deployment profile hash, and both code hashes. Gate B also sets `parent_gate_a_manifest_sha256` to that Gate A hash.

`release-manifest.json` has the following shape. Hashes are lowercase or uppercase 64-digit SHA-256 values. Timestamps are Unix seconds and the validity window must not exceed 90 days.

```json
{
  "schema_version": 2,
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

The actual Gate A manifest must list all four Gate A artifacts exactly once; Gate B must list all nine Gate B artifacts exactly once. Paths must be single relative file paths; symlinks or path traversal outside the bundle are rejected. Gate A omits the parent hash; Gate B requires it. Manifest release-approver signatures and key ceremony artifacts do not exist.

`signer-snapshot.json` schema v2 records the already-observed live values and a normalized `public_config` object. Gate B compares every profile-bound PublicConfig field, including contracts/canisters, stable schema, signer/operator, RPC digest, rate limits, gas/fee/liveness policy, reserve policy, governance/pause principals and fee recipient. The observation must be no older than five minutes. The Timelock delay is an exact profile match, not merely a 72-hour lower bound.

`monitor-drill.json` schema v3 binds the emergency pause claim to exact Finalized Base actions and an IC request certificate. Offline validation recomputes response, certificate and audit digests and enforces the exact pause/cancel action set. `verify-gate-a-live` additionally verifies the mainnet IC certificate and certified reply semantics, then requires 2-of-3 canonical Finalized agreement for every Base receipt, calldata and event log. Receipt、deployment、保存snapshot、Timelock role eventのblock hashは、番号指定のfull block再取得ではなく、対象contractへのEIP-1898 `eth_call`と`requireCanonical=true`で検証する。

`rpc-e2e.json` is the complete manifest produced and verified by `scripts/evm-rpc-rehearsal/rehearsal.py`; a boolean summary is not accepted. The reviewed verifier source is embedded in the `bridge-profile` binary, so working-directory script replacement cannot alter Gate B. Gate B requires all ten scenarios, raw command artifacts, Canister `EvmRpcObservation` bindings (Canister ID, production call method, internal request/quorum digests, Finalized hash and transaction hash), a fixed module-hash capture, transaction and Ledger references, canonical Finalized confirmation, quorum-loss fail-closed, NonceTooLow handling, and final pause. Provider別全responseとexact agreeing countはEVM RPC client APIの保証境界外なので自己申告せず、configured count、required threshold、故障注入artifact、継続/fail-closed decisionをthreshold certificateとして検証する。Its source revision/tree, RPC URL digests, and Bridge Wasm/runtime hashes must match the release bundle. `monitor-drill.json` records the approved routing hash, one fault origin and the detect, human acknowledgement, Base pause and IC pause timestamps plus public pause references. Every observation/capture must predate the release manifest and be no older than 90 days.

`controller-handover.json` schema v2 is durably reserved before the fixed ICP CLI command. An ambiguous command failure is retained as `controller_update_uncertain`, a submitted request awaiting a readable public postcondition is retained as `controller_update_submitted`, and only a Root-only public controller result becomes `complete`. Gate B accepts only `complete`; it binds the request ID, exact argv, exit code, stdout/stderrのraw bytes、combined response digest, executing principal, cycles balance, freezing requirement, and the Root-only final controller set. Validatorはraw transcriptからresponse digestとrequest IDを再導出し、自由記述の成功要約だけを証跡にしない。`sns-upgrade.json` schema v3 retains the exact authenticated `get_proposal` response. Gate B re-queries it with query-signature verification and independently reads controller/module state before accepting the upgrade.

Activation submission and receipt files are deliberately outside the fixed nine-artifact Gate B bundle. A schema v3 submission records the fixed generic-function proposal only; it is not success evidence. A schema v3 activation receipt is created with exclusive-create semantics only after an authenticated executed proposal, exact live function registry target, Root-only controller/module binding, authenticated `get_activation_status`, and 2-of-3 Finalized Base Timelock postcondition all agree. Execute receipts additionally hash-bind the prior verified schedule receipt.

Do not put a seed, private key, backup, device serial, API token, credential-bearing URL, or other secret in this directory.
