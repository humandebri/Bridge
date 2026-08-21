# Evidence bundle v2

Gate A contains exactly these manifest-bound artifacts:

- `profile.json`
- `bridge-canister.wasm`
- `bridge-runtime.bin`
- `bsns-creation.bin`
- `bsns-runtime.bin`
- `bsns-runtime-layout.json`

Gate Aの`profile.json`は、まだ存在しないreceipt blockを自己申告せず`deployment_block: 0`とする。
paused deployment後、release commandが実receiptから`gate-a-receipt.json`と`<receipt>.post-deploy-profile.json`を同時生成する。
後者だけをGate Bの`profile.json`として使用し、receiptの`post_deploy_profile_sha256`と実Bridge deployment blockを一致させる。Gate B validatorはprofileを複製して`deployment_block`だけを0へ戻したRFC 8785 canonical hashを`gate_a_profile_sha256`と照合するため、その他のfield driftは署名し直しても拒否される。
Gate B uses a new manifest containing those six files plus `rpc-e2e.json`, `gate-a-receipt.json`, `controller-handover.json`, `sns-upgrade.json`, `monitor-drill.json`, `keeper-drill.json`, `monitoring-receipt.json`, `fee-cycles-measurements.json`, `provider-independence.json`, and `ui-assets.json`. The receipt binds the Gate A manifest hash, release/source identity, Gate A profile hash, post-deployment profile hash, and both code hashes. Gate B also sets `parent_gate_a_manifest_sha256` to that Gate A hash.

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

The actual Gate A manifest must list all six Gate A artifacts exactly once; Gate B must list all sixteen Gate B artifacts exactly once. Paths must be single relative file paths; symlinks or path traversal outside the bundle are rejected. Gate A omits the parent hash; Gate B requires it. Manifest release-approver signatures and key ceremony artifacts do not exist.

Gate B reads PublicConfig and reserve status through signature-verified Canister queries. Governance must run `governance-relayer refresh-attestation` immediately before verification; the authenticated Finalized Base attestation must postdate the Gate B manifest and be no older than five minutes. It binds Bridge, Timelock, BSNS, pause, signer, runtime, role and fee state, including an exact Timelock delay match.

`monitor-drill.json` schema v4 binds the emergency pause claim to exact Finalized Base actions and an IC request certificate. Gate Aでは認可入力にせず、Gate Bでresponse、certificate、audit digest、timestamp ordering、exact pause/cancel action setを検証する。staging monitor drillの直接RPC照合だけは`production-live-preflight.sh verify-monitor-drill`を使い、本番Base状態は公式EVM RPC Canister経由で保存したactivation attestationを正本とする。

Gate Bの`monitoring-receipt.json` schema v1は、source、Bridge Canister、Withdrawal ID、burn transaction、Finalized block identity、`WithdrawalCommitted` event、署名検証付き`get_withdrawal`応答の`Paid` stateを一つのartifactへ束縛する。`keeper-drill.json`のdigestはこのartifactの実SHA-256と一致しなければならず、`verify-live`はBase 2-of-3 canonical Finalized receiptとlive Canister queryを再検証する。

`rpc-e2e.json` schema v2 is the manifest produced and verified by `scripts/evm-rpc-rehearsal/rehearsal.py`; a boolean summary is not accepted. Gate B requires raw artifacts for `preflight`, `authorization_mint`, `withdrawal_release`, `quorum_loss`, and `final_pause`, producing `LAUNCH_READY`. `bridge-profile`が呼ぶBase monitor verifierはrepository scriptなので、認可実行はclean source/treeを再検査するproduction wrapper経由に限定し、dirty working treeからのstandalone `verify-live`を認可結果に使わない。`preflight`はreview済みprovider index 0、1、2それぞれのchain ID transport artifactを1件ずつ要求し、1件でも欠落・到達失敗・期待chain不一致ならfail closedとする。この稼働前検証とruntime quorumの役割分離は[ADR 0024](../../docs/adr/0024-validate-rpc-chain-binding-before-runtime.md)に従う。The other five scenarios remain available and produce `EXTENDED_COMPLETE` when recorded before `final_pause`, but do not block activation. Provider別全responseとexact agreeing countはEVM RPC client APIの保証境界外なので自己申告せず、configured count、required threshold、故障注入artifact、継続/fail-closed decisionをthreshold certificateとして検証する。Its source revision/tree, RPC URL digests, and Bridge Wasm/runtime hashes must match the release bundle. `monitor-drill.json` records the approved routing hash, one fault origin and the detect, human acknowledgement, Base pause and IC pause timestamps plus public pause references. Every observation/capture must predate the release manifest and be no older than 90 days.

`controller-handover.json` schema v2 is durably reserved before the fixed ICP CLI command. An ambiguous command failure is retained as `controller_update_uncertain`, a submitted request awaiting a readable public postcondition is retained as `controller_update_submitted`, and only a Root-only public controller result becomes `complete`. Gate B accepts only `complete`; it binds the request ID, exact argv, exit code, stdout/stderrのraw bytes、combined response digest, executing principal, cycles balance, freezing requirement, and the Root-only final controller set. Validatorはraw transcriptからresponse digestとrequest IDを再導出し、自由記述の成功要約だけを証跡にしない。`sns-upgrade.json` schema v3 retains the exact authenticated `get_proposal` response. Gate B re-queries it with query-signature verification and independently reads controller/module state before accepting the upgrade.

Activation submission and receipt files are deliberately outside the fixed sixteen-artifact Gate B bundle. A schema v3 submission records the fixed generic-function proposal only; it is not success evidence. A schema v3 activation receipt is created with exclusive-create semantics only after an authenticated executed proposal, exact live function registry target, Root-only controller/module binding, authenticated `get_activation_status`, and the Canister's independently confirmed Finalized Base transaction all agree. Execute receipts additionally hash-bind the prior verified schedule receipt.

Do not put a seed, private key, backup, device serial, API token, credential-bearing URL, or other secret in this directory.
