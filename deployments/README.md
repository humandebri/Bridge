# Deployment profiles

`bridge-profile`は秘密を含まないJSON profileと計測evidenceを検査する。

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
cargo run -p bridge-profile -- validate-test rehearsal-profile.json
cargo run -p bridge-profile -- validate-bundle --offline evidence/release-id
cargo run -p bridge-profile -- verify-live evidence/release-id
```

`derive`はgasとcyclesを各operation 100件、Base feeの30日sampleが揃わなければ失敗する。Mint limitとwindow長はderiveせず、profileへraw unitで明示する。通常デプロイ前に使う`validate`は`test_assets_only = true`を必ず拒否する。Sepolia rehearsalだけが明示的な`validate-test`を使える。

本番配置と資産受付開始は、必ず`production-release.sh`を経由する。`deploy`はGate Aのoffline bundle検証を通した後だけ配置コマンドを実行する。Bridge contractとBridge Canisterはいずれも初期pause状態で配置され、この段階では資産を受け付けない。

```sh
scripts/production-release.sh deploy --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json -- scripts/production-deploy-driver.sh
```

配置後は、Canisterをpauseしたままprofile記載のSNS Rootへcontroller handoverし、その結果をlive snapshotで確認する。`activate`はGate Bのlive検証と最終署名をdriver自身でも再検証し、明示的な資産受付確認が指定された場合だけ固定Timelock操作を行う。任意のunpause commandは受け付けない。

```sh
BRIDGE_ACTIVATION_PHASE=schedule scripts/production-release.sh activate --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json \
  --confirm-asset-acceptance UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE -- scripts/production-activate-driver.sh
```

bundle欠落、test profile、source/profile drift、Gate失敗、署名欠落では後続コマンドを起動しない。Gate Aのdeployコマンドにunpauseまたはresume操作を混在させることも拒否する。
Gate A receiptはoffline manifest、clean source、profileのhashを固定する。Gate Bは別のlive manifestとし、`parent_gate_a_manifest_sha256`がreceiptのGate A hashと一致し、source/profile/code bindingも同一でなければならない。Gate B bundleの署名前に固定`production-live-preflight.sh capture BUNDLE OUTPUT`でfresh snapshotを生成する。activation直前の`verify`は同じheight/hashとlive stateを再照合し、署名済みbundle自体は変更しない。
外部`--receipt`はGate B bundle内の`gate-a-receipt.json`とbyte単位で一致しなければならない。

固定driverはhardware walletを使う`forge`/`cast`とreview済み`dfx` identityだけを許可し、live preflightはprofileに束縛された3 RPC、Base contract、Timelock、IC Canisterをread-only照会する。必要な外部環境やceremony入力がなければfail closedとなり、任意scriptや`/bin/true`への差替えはwrapperが拒否する。Timelock activationは、fresh Gate Bで`BRIDGE_ACTIVATION_PHASE=schedule`を実行し、72時間後に新しいsnapshot・署名・Gate B bundleを作って`BRIDGE_ACTIVATION_PHASE=execute`を実行する二段階である。`execute`にはprofileと一致する`BRIDGE_RUNTIME_ADMIN_ADDRESS`も必須であり、IC resumeが失敗した場合はICを再pauseしてからこのhardware walletでBase両flowも再pauseして確認し、常にincidentとして終了する。

Gate A deploy driverは外部指定のCanister initやconstructor JSONを使用しない。固定sourceからbuildした`bridge-profile`でbundle内profileを一時directoryへ再生成し、その生成物だけをCanister installとcontract deploymentへ渡す。

profileはCanisterから導出してBaseのSafe snapshotと照合する`expected_bridge_signer`、公式EVM RPC Canister ID、release approver、Wasm/bytecode hash、72時間Timelock、独立canceller、固定limit、fee/reserve関係を含む。3件のRPC providerはcredentialなしのHTTPS URLであり、URL文字列が互いに異ならなければならない。URL pathは公開allowlistだけを許可し、userinfo、query、fragment、opaque pathを拒否する。providerの運営主体や基盤の独立性は監査しない。監視欄は通知routingのSHA-256と、検知5分、担当確認15分、Base/IC双方pause 60分のSLOを正確に記録する。資産・fee・gas・reserveの`u128`値はJCSの数値丸めを避けるためcanonical decimal stringで記録する。

Gate Aの`release-manifest.json`は`profile.json`、`ceremony.json`、`monitor-drill.json`、`bridge-canister.wasm`、`bridge-runtime.bin`をSHA-256で束縛する。配置後に別のGate B manifestを作り、`signer-snapshot.json`、`rpc-e2e.json`と`gate-a-receipt.json`を加える。Gate Bは`parent_gate_a_manifest_sha256`、同じrelease/source/profile/code hashを照合する。有効期間は最大90日である。JSON hashは浮動小数を禁止したRFC 8785互換subsetでcanonicalizeする。Gate Bではapprovalを除くmanifest hashの32 byte値に対するEIP-191署名をrecoverし、profileの`release_approver`と一致させる。自己申告の`status: "validated"`は使用しない。

`validate-bundle --offline`はartifact、profile、ceremony、5/15/60監視演習と、署名が存在する場合は署名を検査する。`verify-live`はoffline検査を内包し、production bundle、署名、取得済みsnapshotのchain/canonical quorum/signer/code/Timelock/controller/reserve、およびmockを使わないEVM RPC rehearsal証跡を検査する。CLI自体はnetwork requestを行わない。snapshot取得、current source revision/treeとの比較、Gate A receiptとのbindingはproduction wrapperが行う。

credential、seed、private key、hardware wallet backup、credential入りRPC URLはprofileやevidenceへ記録しない。`ceremony.json`の`contains_secret_material`がtrueなら拒否する。

## ICP mainnet Bridge deploy先

暫定deploy先は`rlhjx-iyaaa-aaaaf-qcnyq-cai`とする。2026年7月14日のpreflightではWasm未インストール（`module_hash = null`）で、controllerは`production` identityだった。

deploy前に対象IDとcontrollerを再確認し、必要なcyclesを補充する。初期検証が完了するまで本番資産を受け付けず、本番資産の受付前にSNS Rootを唯一のcontrollerとしてhandoverする。

## Base Sepolia contract-only experiment

[`scripts/base-sepolia-experiment/`](../scripts/base-sepolia-experiment/)は、固定limit版Bridgeと72時間Timelockの実transaction検証を段階実行する。
再開手順と秘密情報の扱いは[`docs/runbooks/base-sepolia-rehearsal.md`](../docs/runbooks/base-sepolia-rehearsal.md)に記録する。

作業中の公開manifestは[`base-sepolia-contract-experiment.json`](base-sepolia-contract-experiment.json)であり、スクリプトがstate、transaction、confirmationを更新する。
各回の公開スナップショットは`deployments/base-sepolia/YYYY-MM-DD/manifest.json`へ保存し、未実行項目を推測値で埋めない。
2026年7月13日の記録は[`base-sepolia/2026-07-13/manifest.json`](base-sepolia/2026-07-13/manifest.json)を参照する。

実験用deployerがBase Admin walletとRuntime Administratorを兼任するため、本番role分離の証跡としては使用しない。
private key、seed、keystore password、credential付きRPC URLは保存しない。

## EVM RPC Canister経由の実演習

[`scripts/evm-rpc-rehearsal/`](../scripts/evm-rpc-rehearsal/)は、IC上のtest Bridgeから公式EVM RPC Canisterを経由するBase Sepolia実演習の証跡をfail closedで記録する。
通常CIは外部transactionを送信せず、recorderとlive-only guardだけを検査する。
実行条件、scenario、秘密情報の扱いは[`docs/runbooks/evm-rpc-canister-rehearsal.md`](../docs/runbooks/evm-rpc-canister-rehearsal.md)を参照する。

公式Canisterとprovider quorumがcanonical Safe chainを返すことは外部仮定として証跡に残す。
