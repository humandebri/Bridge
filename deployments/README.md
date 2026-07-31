# Deployment profiles

`bridge-profile`は秘密を含まないJSON profileと計測evidenceを検査する。

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
cargo run -p bridge-profile -- validate-test rehearsal-profile.json
cargo run -p bridge-profile -- validate-bundle --offline evidence/release-id
cargo run -p bridge-profile -- verify-gate-a-live evidence/release-id
cargo run -p bridge-profile -- verify-live evidence/release-id
```

`derive`はDeposit mint gasとsettlement cyclesを各100件、開始・終了時刻で30日以上のBase fee sample、pause時の基礎日次cycles、承認済み日次settlement上限が揃わなければ失敗する。cycles floorは30日負荷モデルの2倍、settlement ceilingは100回最大値の1.5倍切り上げである。Mint limitとwindow長はderiveせず、profileへraw unitで明示する。通常デプロイ前に使う`validate`は`test_assets_only = true`を必ず拒否する。Sepolia rehearsalだけが明示的な`validate-test`を使える。

本番配置と資産受付開始は、必ず`production-release.sh`を経由する。`deploy`はoffline構造検査に加え、監視演習のIC certificate、emergency pause reply/audit、Base receipt/logをrepository-owned verifierで検証する。Base側はbundleへ束縛された3つのcredential-free RPC URLを`BRIDGE_GATE_A_RPC_URL_1`〜`3`として渡し、2-of-3の同一Finalized結果を要求する。Bridge contractとBridge Canisterはいずれも初期pause状態で配置され、この段階では資産を受け付けない。

```sh
scripts/production-release.sh deploy --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json -- scripts/production-deploy-driver.sh
```

配置後は、Canisterをpauseしたままprofile記載のSNS Rootへcontroller handoverし、その結果をlive snapshotで確認する。`activate`はGate Bのlive検証を再実行し、SNS function registryから固定`schedule_activation` / `execute_activation` targetを解決して提案を1件だけ提出する。提出成功はactivation完了を意味しない。`verify-activation`が、認証済みSNS proposal、function registry、Canister module/controllerとactivation状態、Base Timelockの2-of-3 Finalized postconditionをすべて照合して初めて検証済みreceiptを発行する。任意のunpause commandは受け付けない。

```sh
scripts/production-release.sh activate --phase schedule --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json \
  --submission evidence/activation/schedule-submission.json \
  --sns-identity proposer --sns-neuron-subaccount 64-hex \
  --sns-proposer-principal principal \
  --confirm-asset-acceptance SCHEDULE_PRODUCTION_ASSET_ACTIVATION \
  -- scripts/production-activate-driver.sh

cargo run -p bridge-profile -- verify-activation schedule evidence/release-id \
  evidence/activation/schedule-submission.json - evidence/activation/schedule-receipt.json
```

24時間後の`execute`はfresh Gate Bを要求し、`--prior-schedule-receipt`と`UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE`を必須とする。release wrapperはproposal提出前に`verify-schedule-receipt-live`を実行し、receipt内部digest、認証済みSNS/Canister状態、canonical Finalized Base Timelock pending状態が一致しなければ停止する。execute proposalの後も、`verify-activation execute ... schedule-receipt.json execute-receipt.json`が成功するまで資産受付開始を完了扱いにしない。

bundle欠落、test profile、source/profile drift、Gate失敗では後続コマンドを起動しない。Gate Aのdeployコマンドにunpauseまたはresume操作を混在させることも拒否する。
Gate A profileの`deployment_block`は未配置を示す`0`に固定する。deploy後、wrapperは実receipt blockを入れた`<receipt>.post-deploy-profile.json`を生成し、そのSHA-256をGate A receiptへ固定する。Gate Bはこのpost-deploy profileだけを使う別のlive manifestとし、`parent_gate_a_manifest_sha256`がreceiptのGate A hashと一致し、source/code binding、post-deploy profile hash、実deployment blockが一致しなければならない。さらにGate B profileの`deployment_block`だけを0へ戻したcanonical hashがreceiptのGate A profile hashと一致する必要があり、他fieldの変更は拒否される。Gate B bundle確定前に固定`production-live-preflight.sh capture BUNDLE OUTPUT`でfresh snapshotを生成する。activation直前の`verify`は同じheight/hashとlive stateを再照合し、確定済みbundle自体は変更しない。
外部`--receipt`はGate B bundle内の`gate-a-receipt.json`とbyte単位で一致しなければならない。

初回contract deployだけ一時EOAを使用し、deployerへroleを残さない。以後のBase管理操作はBridge Canisterが導出するGovernance Operatorだけから送信する。production IC操作は`BRIDGE_ICP_IDENTITY`とICP CLIへ統一し、`dfx`を使用しない。Timelock activationはSNS proposalからCanisterの固定schedule/execute APIを呼ぶ二段階とし、各段階でlive preflightを再実行する。失敗または曖昧結果ではIC/Base pauseを維持し、同じsigned transactionを追跡する。

production CanisterはGate A確定前にpause状態でinstallし、固有のMint SignerとGovernance Operatorをprofileへ固定する。Gate A deploy driverは外部指定のconstructor JSONを使用せず、固定sourceからbuildした`bridge-profile`でbundle内profileを一時directoryへ再生成し、稼働中Canisterの2 addressとpause状態を照合してからcontract deploymentへ渡す。Canisterの再installやdeployment binding APIは実行しない。

profileはCanisterから導出してBaseのFinalized snapshotと照合するMint SignerとGovernance Operator、current stable schema、公式EVM RPC Canister ID、単一emergency pause principal、Wasm/bytecode hash、Timelock、固定limit、fee/liveness/reserve関係を含む。Timelock delayはprofileとlive stateの完全一致を要求する。`timelock.runtime_code_hash`は`0x`付き32-byte Keccak runtime code hashであり、生成されたBridge constructor引数、配置直後の実code hash、Gate B Finalized snapshotの三者が一致しなければならない。配置後にGate A receiptがBridge/Timelockのcanonical deployment transaction・blockを記録し、Gate B snapshotが3 providerで再照合する。既知のreceipt、deployment、snapshot、role-event block hashはEIP-1898 `eth_call` probeでcanonicalityを確認し、full block取得は未知hashを発見するFinalized headに限定する。3件のproduction Base RPC providerはcredentialなしのHTTPS URLであり、URL文字列が互いに異ならなければならない。監視欄は通知routingのSHA-256と、検知5分、担当確認15分、Base/IC双方pause 60分のSLOを正確に記録する。

Gate Aはpre-deploy `profile.json`、`monitor-drill.json`、`bridge-canister.wasm`、`bridge-runtime.bin`の正確に4 artifactを束縛する。Gate Bはこれらへ`signer-snapshot.json`、`rpc-e2e.json`、`gate-a-receipt.json`、`controller-handover.json`、`sns-upgrade.json`を加えた正確に9 artifactである。release approver署名と鍵ceremonyは使用しない。Mint Signerはprofile、Canister公開設定、Finalized Base stateの三者一致で検証する。x402はBridgeの配置・activation条件ではない。

`validate-bundle --offline`は構造検査だけを行い、出力にも`authorizing=false`を明記する。Gate Aの認可判定は`verify-gate-a-live`だけであり、IC certificateとBaseの2-of-3 Finalized receipt/logを検証する。`verify-live`はGate Bの構造・fresh snapshotに加え、SNS upgrade proposalを認証済みqueryで再取得し、Root-only controllerとlive module hashをread-stateで照合する。これらのlive commandはネットワークへ接続し、認証・合意・postconditionのいずれかが欠ければ非ゼロ終了する。

credential、seed、private key、hardware wallet backup、credential入りRPC URLはprofileやevidenceへ記録しない。

## IC mainnet × Base Sepolia test staging

Plan 007のIC stagingで新規作成またはinstallするCanisterは`sepolia-staging`環境の`bridge-sepolia`だけとする。test tokenには既存の共有`testicrc` Canisterを再利用し、staging専用LedgerまたはIndex Canisterを新規作成しない。`testicrc`の実Canister IDとmetadataは外部配置前にlive状態から確認し、Bridge初期化値と`frontend-profile.json`へ固定する。test frontendはIC Asset Canisterへ配置せず、静的assetをCloudflare Worker `kinic-bridge-ui-test`から配信する。`rlhjx-iyaaa-aaaaf-qcnyq-cai`は未配置の旧production候補からstaging専用Bridgeへ再分類し、production mappingから除外する。KINIC Ledger、Base Mainnet、SNSには触れない。

外部配置前にリポジトリ直下の`scripts/plan007-local-gate.sh`をclean commitで実行し、`deployments/sepolia-staging/evidence/local-e2e.json`を発行する。dirty treeまたはhash driftでは証跡を発行しない。外部deploy、cycles投入、Base Sepolia transaction、Cloudflare Worker公開はそれぞれ別の明示承認後に行う。

外部stageの証跡は`scripts/plan007/staging-e2e-driver.sh`で初期化し、固定順序で記録する。`sepolia-e2e.json`は全stageと参照artifactのhashを検証でき、`COMPLETE`には実wallet matrix、10件のRPC rehearsal、同一Wasm upgrade、Base/ICのfinal pause、pending settlement/Timelockゼロが必要である。

新規作成するstaging Bridge Canister IDだけを`.icp/data/mappings/sepolia-staging.ids.json`へ保存する。既存`testicrc`を新規作成対象としてmappingへ追加しない。frontendは`deployments/sepolia-staging/frontend-profile.json`が完成するまでbuildまたは公開せず、完成後に`ui`の`pnpm run deploy:test`でCloudflare Worker `kinic-bridge-ui-test`へ公開する。test frontendはBase Mainnet、production Canister ID、非公式EVM RPC Canister IDを拒否し、TEST bannerを常時表示する。

## ICP mainnet上のBase Sepolia staging Bridge deploy先

staging deploy先は`rlhjx-iyaaa-aaaaf-qcnyq-cai`とする。2026年7月22日のpreflightではWasm未インストール（`module_hash = null`）で、controllerは`production` identityだった。このIDは`.icp/data/mappings/sepolia-staging.ids.json`の`bridge-sepolia`にだけ割り当て、production環境は未割当のままにする。

deploy前に対象IDとcontrollerを再確認し、必要なcyclesを補充する。test-only stagingであり、本番資産、production controller handover、SNS操作には使用しない。

## Base Sepolia contract-only experiment

[`scripts/base-sepolia-experiment/`](../scripts/base-sepolia-experiment/)は、固定limit版Bridgeと72時間Timelockの実transaction検証を段階実行する。
再開手順と秘密情報の扱いは[`docs/runbooks/base-sepolia-rehearsal.md`](../docs/runbooks/base-sepolia-rehearsal.md)に記録する。

作業中の公開manifestは`base-sepolia-contract-experiment.json`へスクリプトが新規生成する。旧Canister発Mint ABIで作成された作業用manifestは再利用せず、EIP-712対応Bridgeの再deploy演習から作り直す。
各回の公開スナップショットは`deployments/base-sepolia/YYYY-MM-DD/manifest.json`へ保存し、未実行項目を推測値で埋めない。
2026年7月13日の記録は[`base-sepolia/2026-07-13/manifest.json`](base-sepolia/2026-07-13/manifest.json)を参照する。この記録は旧Canister発Mint ABIの履歴証跡であり、現行deploy、preflight、release evidenceには使用しない。

実験用deployerがBase Admin walletとRuntime Administratorを兼任するため、本番role分離の証跡としては使用しない。
private key、seed、keystore password、credential付きRPC URLは保存しない。

## EVM RPC Canister経由の実演習

[`scripts/evm-rpc-rehearsal/`](../scripts/evm-rpc-rehearsal/)は、IC上のtest Bridgeから公式EVM RPC Canisterを経由するBase Sepolia実演習の証跡をfail closedで記録する。
通常CIは外部transactionを送信せず、recorderとlive-only guardだけを検査する。
実行条件、scenario、秘密情報の扱いは[`docs/runbooks/evm-rpc-canister-rehearsal.md`](../docs/runbooks/evm-rpc-canister-rehearsal.md)を参照する。

公式Canisterとprovider quorumがcanonical Finalized chainを返すことは外部仮定として証跡に残す。
