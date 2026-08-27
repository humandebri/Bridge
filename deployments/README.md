# Deployment profiles

`bridge-profile`は秘密を含まないJSON profileと計測evidenceを検査する。

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
cargo run -p bridge-profile -- validate-test rehearsal-profile.json
cargo run -p bridge-profile -- validate-bundle --offline evidence/release-id
cargo run -p bridge-profile -- verify-live evidence/release-id
```

`derive`はschema v2のgovernance gas、settlement cycles、各fee系列を10件以上、開始・終了時刻で7日以上のBase fee sample、pause時の基礎日次cycles、承認済み日次settlement上限が揃わなければ失敗する。cycles floorは30日負荷モデルの2倍、settlement ceilingは10回以上のsample最大値の1.5倍切り上げである。Mint limitとwindow長はderiveせず、profileへraw unitで明示する。通常デプロイ前に使う`validate`は`test_assets_only = true`を必ず拒否する。Sepolia rehearsalだけが明示的な`validate-test`を使える。

本番配置と資産受付開始は、必ず`production-release.sh`を経由する。`deploy`のGate Aはoffline artifact、profile、constructor条件だけを検証する。Bridge contractとBridge Canisterはいずれも初期pause状態で配置され、この段階では資産を受け付けない。配置後のruntime、role、pause、chain bindingは、Canisterが公式EVM RPC Canisterの組み込み`BaseMainnet`から取得して保存するactivation attestationをGate Bで検証する。

production CanisterはBase contract用release profileとは独立したschema 1の`production-canister-plan.json`から一度だけinstallする。`deployments/production-canister-plan.template.json`をrepo外へ複製し、bootstrapで確定したCanister ID、clean source、Wasm、初期設定を埋める。`scripts/production-canister-install.sh`だけがtyped planをCandid binaryへ変換し、`--mode install --args-format bin`でinstallする。`reinstall`、`auto`、暗黙buildは使用しない。public config初期化、全storage検査、checksum、Bootstrap lifecycle、空state、pause、cycles reserve、RuntimeBinding、controller/module hashの全postconditionを満たしたschema 1 receiptだけを後続profileの根拠にする。

```sh
scripts/production-release.sh deploy --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --canister-install-receipt evidence/production-canister-install.json \
  --receipt evidence/release-id/gate-a-receipt.json -- scripts/production-deploy-driver.sh
```

配置後は、Canisterをpauseしたまま運用設定を一度だけ封印し、profile記載のSNS Rootへcontroller handoverして、その結果をlive snapshotで確認する。handover driverには配置完了後のschema 2 Gate A receipt、その内部に埋め込まれたものと完全一致するinstall receipt、Gate A wrapperが使ったcanonical deployment bindingを渡す。Gate A receiptが未生成、bindingのdeployer/nonce/address/transaction/blockがreceipt・profileと一致しない、またはreceipt間にdriftがある場合は不可逆なcontroller変更前に拒否する。封印時にCanisterが公式EVM RPC Canisterの組み込み`BaseMainnet`から生成したfresh activation attestationを認証済みqueryで取得し、`OperationalConfigSealed` lifecycle、profileのchain/address/runtime/role/delay/BSNS/fee/pause、両deployment block以後のFinalized観測へ完全一致させる。公開RuntimeBindingのempty RPC digestと`operational_config_sha256`もprofileのgovernance EVM fee、cycles floor、settlement cycle ceilingを含む全運用設定から再構成し、pause/reserve/空stateと併せて照合する。同時にcertified `read_state`のmodule hashとinstaller単独controllerをinstall receiptへ再照合する。直接Custom RPC 3本を使うのはstaging monitor drillだけであり、本番handoverには使用しない。`activate`はGate Bのlive検証を再実行し、SNS function registryから固定`schedule_activation` / `execute_activation` targetを解決して提案を1件だけ提出する。提出成功はactivation完了を意味しない。`verify-activation`が、認証済みSNS proposal、function registry、Canister module/controller、activation状態、保存済みFinalized activation結果を照合して初めて検証済みreceiptを発行する。任意のunpause commandは受け付けない。

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
Gate A profileの`deployment_block`は未配置を示す`0`に固定する。deploy前にwrapperはCanister install receiptをtyped profileとclean sourceへ照合する。Base transaction送信直前には、そのreceiptもpredeploy verifierへ渡し、certified `read_state`のmodule hashがreceiptとprofileのWasm SHA-256の両方へ一致し、controller集合がreceiptのinstaller principal単独であることを再確認する。deploy後、wrapperは実receipt blockを入れた`<receipt>.post-deploy-profile.json`を生成し、そのSHA-256とinstall receipt全体をschema 2 Gate A receiptへ固定する。Gate Bはこのpost-deploy profileだけを使う別のlive manifestとし、`parent_gate_a_manifest_sha256`がreceiptのGate A hashと一致し、source/code binding、post-deploy profile hash、実deployment block、install時のCanister identity/module/runtime/pauseが一致しなければならない。さらにGate B profileの`deployment_block`だけを0へ戻したcanonical hashがreceiptのGate A profile hashと一致する必要があり、他fieldの変更は拒否される。staging monitor drillの直接RPC検証は`production-live-preflight.sh verify-monitor-drill BUNDLE`だけを使い、本番Base状態の正本にはしない。
外部`--receipt`はGate B bundle内の`gate-a-receipt.json`とbyte単位で一致しなければならない。

初回contract deployだけ一時EOAを使用し、deployerへroleを残さない。以後のBase管理操作はBridge Canisterが導出するGovernance Operatorだけから送信する。production IC操作は`BRIDGE_ICP_IDENTITY`とICP CLIへ統一し、`dfx`を使用しない。Timelock activationはSNS proposalからCanisterの固定schedule/execute APIを呼ぶ二段階とし、各段階でlive preflightを再実行する。失敗または曖昧結果ではIC/Base pauseを維持し、同じsigned transactionを追跡する。

production CanisterはGate A確定前にpause状態でinstallし、固有のMint SignerとGovernance Operatorをprofileへ固定する。Gate A deploy driverは外部指定のconstructor JSONを使用せず、固定sourceからbuildした`bridge-profile`でbundle内profileを一時directoryへ再生成し、稼働中Canisterの2 addressとpause状態を照合してからcontract deploymentへ渡す。Canisterの再installやdeployment binding APIは実行しない。

profileはCanisterから導出してBaseのFinalized attestationと照合するMint SignerとGovernance Operator、current stable schema、公式EVM RPC Canister ID、単一emergency pause principal、Wasm/bytecode hash、Timelock、固定limit、fee/liveness/reserve関係を含む。Timelock delayはprofileとlive stateの完全一致を要求する。`timelock.runtime_code_hash`は`0x`付き32-byte Keccak runtime code hashであり、生成されたBridge constructor引数、配置直後の実code hash、Gate B Finalized attestationの三者が一致しなければならない。配置後にGate A receiptがBridge/Timelockのcanonical deployment transaction・blockを記録し、Gate Bは公式EVM RPC Canister経由でcurrent runtimeとroleを再照合する。監視欄は通知routingのSHA-256と、検知5分、担当確認15分、Base/IC双方pause 60分のSLOを正確に記録する。

Gate Aはpre-deploy profileとBridge/BSNSの5 build artifact、合計6 artifactを束縛する。Canister install receiptは7番目のartifactへ追加せず、schema 2 Gate A receipt内へ完全に埋め込み、Gate Bへ推移的に継承する。Gate BはGate Aの6 artifactへRPC rehearsal、handover、SNS upgrade、monitor/keeper、fee/cycles、provider independence、UI、Gate A receiptの10 artifactを加えた正確に16 artifactである。release approver署名と鍵ceremonyは使用しない。Mint Signerはprofile、認証済みCanister公開設定、freshなFinalized Base attestationの三者一致で検証する。x402はBridgeの配置・activation条件ではない。

`validate-bundle --offline`はGate Aの正式なoffline認可判定として`gate_a=pass authorizing=true`だけを成功出力する。`verify-live`はGate Bの構造に加え、5分以内のactivation attestation、公開RuntimeBinding、reserve、SNS upgrade proposal、Root-only controller、live module hashを認証済みCanister応答で照合する。権限principal、rate/cycles policy、Governance fee、固定Ledger feeは、公開RuntimeBindingの`operational_config_sha256`をrelease profileから再構成した値と照合する。実値の確認はcontroller/governance限定`get_operational_config`を使う。認証またはpostconditionが欠ければ非ゼロ終了する。

credential、seed、private key、hardware wallet backup、credential入りRPC URLはprofileやevidenceへ記録しない。

## IC mainnet × Base Sepolia test staging

Plan 007のIC stagingで新規作成またはinstallするCanisterは`sepolia-staging`環境の`bridge-sepolia`だけとする。test tokenには既存の共有`testicrc` Canisterを再利用し、staging専用LedgerまたはIndex Canisterを新規作成しない。`testicrc`の実Canister IDとmetadataは外部配置前にlive状態から確認し、Bridge初期化値と`frontend-profile.json`へ固定する。test frontendはIC Asset Canisterへ配置せず、静的assetをCloudflare Worker `kinic-bridge-ui-test`から配信する。`rlhjx-iyaaa-aaaaf-qcnyq-cai`は未配置の旧production候補からstaging専用Bridgeへ再分類し、production mappingから除外する。KINIC Ledger、Base Mainnet、SNSには触れない。

外部配置前にリポジトリ直下の`scripts/plan007-local-gate.sh /secure/work/local-e2e.json`をclean commitで実行し、repo外へ証跡を発行する。dirty treeまたはhash driftでは証跡を発行しない。外部deploy、cycles投入、Base Sepolia transaction、Cloudflare Worker公開はそれぞれ別の明示承認後に行う。

外部stageのschema v7証跡は`scripts/plan007/staging-e2e-driver.sh`で初期化し、固定順序で記録する。`sepolia-e2e.json`は全stageと参照artifactのhashを検証する。Gate B用のpause rehearsalはlive staging promotionと分離し、production activationをblockしない。旧staging stackはexact identityと未処理test stateを記録したうえで`abandoned-test-only`としてactive profile、signer、automationから除外する。fresh Timelock、Bridge、bSNS、signer、deployment instance、schema v35／wire v30 Canisterを新規配置し、旧recordやassetを移送しない。migration、reinstall、異なるcurrent-upgrade instance、旧・未知schema、未登録module／Candidの組はfail closedにする。

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
