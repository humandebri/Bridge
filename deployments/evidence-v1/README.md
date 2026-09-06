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
Gate Bの`profile.json`は後者を基礎に、governance EVM fee 8項目、`cycles_floor`、`settlement_cycle_ceiling`だけを`initial-operational-parameters.json`の導出値へ置換する。Gate B validatorはこの10項目をBootstrap値へ正規化して`post_deploy_profile_sha256`を照合し、さらに`deployment_block`を0へ戻したRFC 8785 canonical hashを`gate_a_profile_sha256`へ照合するため、その他のfield driftはmanifestを作り直しても拒否される。
Gate B uses a schema 4 manifest containing exactly the six current release build files plus `initial-operational-parameters.json`, `provider-independence.json`, `ui-assets.json`, `gate-a-receipt.json`, `gate-a-profile.json`, `production-canister-upgrade-receipt.json`, and `post-gate-a-policy-transition.json`. Initial evidence binds exact schedule/execute calldata gas estimates, at least ten distinct Finalized fee blocks, and the live idle cycles burn observation. The validator re-derives the fixed fee/cycles ceilings and requires an exact profile match. The immutable Gate A profile and receipt bind the installed Wasm, while the typed production upgrade evidence is either the original single receipt or an append-only chain of at most 16 raw receipts (256 MiB decoded total). The validator checks every hash link, source revision/tree, signed chunk submission, Wasm transition, sole controller, lifecycle/pause state, schema, runtime, storage integrity, and public-state continuity. Bootstrap and OperationalConfigSealed receipts must remain paused; Activated receipts preserve their observed pause state across each upgrade. The allowed continuity exceptions are the one-time schema 35→36 migration and the one-time production Bootstrap migration from the Gate A SNS Root pause principal to the production controller identity, including their exact combined transition; pause-principal migration remains Bootstrap-only. The immutable Gate A RuntimeBinding must match before those transitions; only the schema version, the derived operational-config digest, and the retained audit count by exactly one may change as applicable, while every other runtime/status field remains fixed. The schema 3 policy transition binds the complete evidence file to the current controller-bootstrap Wasm without changing deployed identities or contract code. The transition records the final upgrade source revision/tree separately from the current release-policy revision/tree; the production wrapper verifies the Gate A → every upgrade → current ancestry and exact source trees, while the current source must reproducibly build the deployed Wasm. Ordinary JSON artifacts remain limited to 16 MiB. Gate B sets `parent_gate_a_manifest_sha256` to the Gate A hash.

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

The actual Gate A manifest must list all six Gate A artifacts exactly once; Gate B must list all thirteen Gate B artifacts exactly once. Paths must be single relative file paths; symlinks or path traversal outside the bundle are rejected. Gate A omits the parent hash; Gate B requires it. Manifest release-approver signatures and key ceremony artifacts do not exist.

Gate B reads the public RuntimeBinding and reserve status through signature-verified Canister queries. The schedule/execute prepare wrapper uses the fixed confirmation relayer identity to refresh the attestation immediately before verification; manual `governance-relayer refresh-attestation` is diagnostic only. The authenticated Finalized Base attestation must postdate the Gate B manifest and be no older than five minutes. It binds Bridge, Timelock, BSNS, pause, signer, runtime, role and fee state, including an exact Timelock delay match. Operational configuration is read separately with a controller or governance identity.

`monitor-drill.json` schema v4 binds the emergency pause claim to exact Finalized Base actions and an IC request certificate. Gate A／Gate Bの認可入力にはせず、unpause後のGate C運用証跡としてresponse、certificate、audit digest、timestamp ordering、exact pause/cancel action setを検証する。staging monitor drillの直接RPC照合だけは`production-live-preflight.sh verify-monitor-drill`を使い、本番Base状態は公式EVM RPC Canister経由で保存したactivation attestationを正本とする。この証跡はcontroller handoverの認可入力ではない。

稼働後のGate Cでは、`rpc-e2e.json`、`monitor-drill.json`、`monitoring-receipt.json`、`keeper-drill.json`を7日・各10件以上の本番計測およびoperation continuity証跡と合わせて要求する。これらはunpauseを認可するGate Bにもcontroller handoverの認可入力にも含めない。

`rpc-e2e.json` schema v2 is the manifest produced and verified by `scripts/evm-rpc-rehearsal/rehearsal.py`; a boolean summary is not accepted. The template and standalone verifier remain available for post-unpause Gate C evidence, but the current schema is not yet admissible: it conflates production-release and rehearsal Wasm hashes, the monitor schema conflates production and rehearsal pause principals, outer staging v8 requires all ten scenarios despite the inner launch-ready split, and the fixed `quorum_loss` injector records `request_deposit` while its validator requires `notify_withdrawal`. The reviewed fixed staging URLs also expose no fault-control API. These inconsistencies must be replaced and reviewed before Gate C collection; they must not be bypassed or used to authorize Gate B, activation, or handover. Quorum-loss safety remains mandatory PocketIC/proof negative evidence. Production bundle freeze retains its no-follow/hash/size validation for later evidence handling.

`controller-handover.json`と`sns-upgrade.json`は、運用者がhandover時期を別途承認した場合に作成する。Gate Bから初回activation完了まではGate A receiptのproduction installer identityを単独controllerとして維持し、Root-only controllerを要求しない。handover認可はGate Cおよび`fee-cycles-measurements.json`と独立し、Gate Bの`initial-operational-parameters.json`、seal receipt、hash接続したschedule／execute receipt、認証済みlive RuntimeBindingへ束縛する。completion receipt自身にもsource revision/tree、Gate B manifest、seal、schedule、executeの各SHA-256を保存し、typed validatorで元の固定bytesへ再結合する。live moduleは初回install hashではなく、`post-gate-a-policy-transition.json`と通常upgrade receiptを経由したcurrent profile Wasmに一致しなければならない。運用者は固定confirmation relayerでattestationを手動refreshしてからhandover driverを実行し、driver自身は更新を行わずfreshな認証済み値だけを受理する。送信直前にproduction installer一件だけのcontroller集合を再取得し、Activated、Base Deposit／Withdrawal unpaused、IC Deposit unpaused、reserve sufficient、storage integrity `ok`を要求する。変更前後のmanagement status、module、RuntimeBinding、lifecycle、activation status／attestation、storage integrity、record／audit countをraw response digest付きで保存して非退行を検証し、空stateは要求しない。

Activation submission and receipt files are deliberately outside the fixed thirteen-artifact Gate B bundle. 初回activationはschema 1のcontroller authorization、prepare、confirmation、completion receiptを使い、production installer単独controller、認証済み`get_activation_status`、Canisterが独立確認したFinalized Base transactionを束縛する。schema v3 generic-function submissionとschema v4 SNS proposal activation receiptは初回activationには使用しない。現行schema v4 verifierはproduction installer controller bindingを残すため、handover後の再activationへ使用する前にRoot-controller／handover-completion bindingへ更新し、別レビューを通す必要がある。両receipt経路ともexecute receiptは検証済みschedule receiptをhash bindingする。

Do not put a seed, private key, backup, device serial, API token, credential-bearing URL, or other secret in this directory.
