# Deployment profiles

`bridge-profile`は秘密を含まないJSON profileと計測evidenceを検査する。

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
cargo run -p bridge-profile -- validate-test rehearsal-profile.json
cargo run -p bridge-profile -- validate-bundle --offline evidence/release-id
cargo run -p bridge-profile -- verify-live evidence/release-id
```

`derive`はDeposit mint gasとsettlement cyclesを各100件、開始・終了時刻で30日以上のBase fee sample、pause時の基礎日次cycles、承認済み日次settlement上限が揃わなければ失敗する。cycles floorは30日負荷モデルの2倍、settlement ceilingは100回最大値の1.5倍切り上げである。Mint limitとwindow長はderiveせず、profileへraw unitで明示する。通常デプロイ前に使う`validate`は`test_assets_only = true`を必ず拒否する。Sepolia rehearsalだけが明示的な`validate-test`を使える。

本番配置と資産受付開始は、必ず`production-release.sh`を経由する。`deploy`はGate Aのoffline bundle検証を通した後だけ配置コマンドを実行するが、repository-ownedなBase receipt/logとIC certificate/auditの真正性検証が実装されるまではGate Aが必ず非ゼロ終了するため、本番配置は利用できない。将来Gate Aが有効になった場合も、Bridge contractとBridge Canisterはいずれも初期pause状態で配置され、この段階では資産を受け付けない。

```sh
scripts/production-release.sh deploy --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json -- scripts/production-deploy-driver.sh
```

配置後は、Canisterをpauseしたままprofile記載のSNS Rootへcontroller handoverし、その結果をlive snapshotで確認する。`activate`はGate Bのlive検証を再実行するが、repository-ownedなSNS proposal提出・実行確認経路が実装されるまでは必ず非ゼロ終了し、成功を報告しない。固定` schedule_activation` / `execute_activation` proposalは別途SNSから提出し、その実行証跡を新しいGate Bへ取り込む。任意のunpause commandは受け付けない。

```sh
BRIDGE_ACTIVATION_PHASE=schedule scripts/production-release.sh activate --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json \
  --confirm-asset-acceptance UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE -- scripts/production-activate-driver.sh
```

bundle欠落、test profile、source/profile drift、Gate失敗では後続コマンドを起動しない。Gate Aのdeployコマンドにunpauseまたはresume操作を混在させることも拒否する。
Gate A profileの`deployment_block`は未配置を示す`0`に固定する。deploy後、wrapperは実receipt blockを入れた`<receipt>.post-deploy-profile.json`を生成し、そのSHA-256をGate A receiptへ固定する。Gate Bはこのpost-deploy profileだけを使う別のlive manifestとし、`parent_gate_a_manifest_sha256`がreceiptのGate A hashと一致し、source/code binding、post-deploy profile hash、実deployment blockが一致しなければならない。さらにGate B profileの`deployment_block`だけを0へ戻したcanonical hashがreceiptのGate A profile hashと一致する必要があり、他fieldの変更は拒否される。Gate B bundle確定前に固定`production-live-preflight.sh capture BUNDLE OUTPUT`でfresh snapshotを生成する。activation直前の`verify`は同じheight/hashとlive stateを再照合し、確定済みbundle自体は変更しない。
外部`--receipt`はGate B bundle内の`gate-a-receipt.json`とbyte単位で一致しなければならない。

初回contract deployだけ一時EOAを使用し、deployerへroleを残さない。以後のBase管理操作はBridge Canisterが導出するGovernance Operatorだけから送信する。production IC操作は`BRIDGE_ICP_IDENTITY`とICP CLIへ統一し、`dfx`を使用しない。Timelock activationはSNS proposalからCanisterの固定schedule/execute APIを呼ぶ二段階とし、各段階でlive preflightを再実行する。失敗または曖昧結果ではIC/Base pauseを維持し、同じsigned transactionを追跡する。

production CanisterはGate A確定前にpause状態でinstallし、固有のMint SignerとGovernance Operatorをprofileへ固定する。Gate A deploy driverは外部指定のconstructor JSONを使用せず、固定sourceからbuildした`bridge-profile`でbundle内profileを一時directoryへ再生成し、稼働中Canisterの2 addressとpause状態を照合してからcontract deploymentへ渡す。Canisterの再installやdeployment binding APIは実行しない。

profileはCanisterから導出してBaseのFinalized snapshotと照合するMint SignerとGovernance Operator、current stable schema、公式EVM RPC Canister ID、単一emergency pause principal、Wasm/bytecode hash、Timelock、固定limit、fee/liveness/reserve関係を含む。Timelock delayはprofileとlive stateの完全一致を要求する。`timelock.runtime_code_hash`は`0x`付き32-byte Keccak runtime code hashであり、生成されたBridge constructor引数、配置直後の実code hash、Gate B Finalized snapshotの三者が一致しなければならない。配置後にGate A receiptがBridge/Timelockのcanonical deployment transaction・blockを記録し、Gate B snapshotが3 providerで再照合する。3件のproduction Base RPC providerはcredentialなしのHTTPS URLであり、URL文字列が互いに異ならなければならない。監視欄は通知routingのSHA-256と、検知5分、担当確認15分、Base/IC双方pause 60分のSLOを正確に記録する。

Gate Aはpre-deploy `profile.json`、`monitor-drill.json`、`bridge-canister.wasm`、`bridge-runtime.bin`の正確に4 artifactを束縛する。Gate Bはこれらへ`signer-snapshot.json`、`rpc-e2e.json`、`gate-a-receipt.json`、`controller-handover.json`、`sns-upgrade.json`を加えた正確に9 artifactである。release approver署名と鍵ceremonyは使用しない。Mint Signerはprofile、Canister公開設定、Finalized Base stateの三者一致で検証する。x402はBridgeの配置・activation条件ではない。

`validate-bundle --offline`はschema v2 artifact、profile、raw response/receiptへ束縛された5/15/60監視演習を構造検査するが、repository-ownedなBase receipt/logとIC certificate/auditの真正性検証が実装されるまでは必ず非ゼロ終了し、Gate A成功を報告しない。`verify-live`もoffline検査とlive snapshot検査を行うが、repository-ownedなSNS certificate/proposalの真正性検証が実装されるまでは必ず非ゼロ終了し、Gate B成功を報告しない。CLI自体はnetwork requestを行わない。

credential、seed、private key、hardware wallet backup、credential入りRPC URLはprofileやevidenceへ記録しない。

## IC mainnet × Base Sepolia test staging

Plan 007のIC stagingは`sepolia-staging`環境と`bridge-sepolia`、`ledger-sepolia`、`index-sepolia`だけを使用する。test frontendはIC Asset Canisterへ配置せず、静的assetをCloudflare Worker `kinic-bridge-ui-test`から配信する。`rlhjx-iyaaa-aaaaf-qcnyq-cai`は未配置の旧production候補からstaging専用Bridgeへ再分類し、production mappingから除外する。KINIC Ledger、Base Mainnet、SNSには触れない。

外部配置前にリポジトリ直下の`scripts/plan007-local-gate.sh`をclean commitで実行し、`deployments/sepolia-staging/evidence/local-e2e.json`を発行する。dirty treeまたはhash driftでは証跡を発行しない。外部deploy、cycles投入、Base Sepolia transaction、Cloudflare Worker公開はそれぞれ別の明示承認後に行う。

staging Canister IDは`.icp/data/mappings/sepolia-staging.ids.json`だけへ保存する。frontendは`deployments/sepolia-staging/frontend-profile.json`が完成するまでbuildまたは公開せず、完成後に`ui`の`pnpm run deploy:test`でCloudflare Worker `kinic-bridge-ui-test`へ公開する。test frontendはBase Mainnet、production Canister ID、非公式EVM RPC Canister IDを拒否し、TEST bannerを常時表示する。

## ICP mainnet上のBase Sepolia staging Bridge deploy先

staging deploy先は`rlhjx-iyaaa-aaaaf-qcnyq-cai`とする。2026年7月22日のpreflightではWasm未インストール（`module_hash = null`）で、controllerは`production` identityだった。このIDは`.icp/data/mappings/sepolia-staging.ids.json`の`bridge-sepolia`にだけ割り当て、production環境は未割当のままにする。

deploy前に対象IDとcontrollerを再確認し、必要なcyclesを補充する。test-only stagingであり、本番資産、production controller handover、SNS操作には使用しない。

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

公式Canisterとprovider quorumがcanonical Finalized chainを返すことは外部仮定として証跡に残す。
