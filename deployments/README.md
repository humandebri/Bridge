# Deployment profiles

`bridge-profile`は秘密を含まないJSON profileと計測evidenceを検査する。

```sh
cargo run -p bridge-profile -- derive measurements.json
cargo run -p bridge-profile -- validate profile.json
cargo run -p bridge-profile -- validate-test rehearsal-profile.json
cargo run -p bridge-profile -- validate-bundle --offline evidence/release-id
cargo run -p bridge-profile -- verify-live schedule evidence/release-id
```

24時間後のexecute認可では同じ位置のphaseを`execute`に替える。

`derive`と`fee-cycles-measurements.template.json`はunpause後のGate C計測用である。schema v3のgovernance gas、settlement cycles、fee系列を各10件以上要求し、Base fee sampleは最初と最後の観測が7日以上離れていなければ失敗する。この7日計測をGate B、初期seal、schedule、executeの前提にしてはならない。Gate Bの初期値は別の`initial-operational-parameters.json`から導出する。通常デプロイ前に使う`validate`は`test_assets_only = true`を必ず拒否し、Sepolia rehearsalだけが明示的な`validate-test`を使える。

本番配置と資産受付開始は、必ず`production-release.sh`を経由する。`deploy`のGate Aはoffline artifact、profile、constructor条件だけを検証する。Bridge contractとBridge Canisterはいずれも初期pause状態で配置され、この段階では資産を受け付けない。配置後のruntime、role、pause、chain bindingは、Canisterが公式EVM RPC Canisterの組み込み`BaseMainnet`から取得して保存するactivation attestationをGate Bで検証する。

production CanisterはBase contract用release profileとは独立したschema 2の`production-canister-plan.json`から一度だけinstallする。`deployments/production-canister-plan.template.json`をrepo外へ複製し、bootstrapで確定したCanister ID、clean source、Wasm、初期設定を埋める。`scripts/production-canister-install.sh`だけがtyped planをCandid binaryへ変換し、`--mode install --args-format bin`でinstallする。`reinstall`、`auto`、暗黙buildは使用しない。public config初期化、全storage検査、checksum、Bootstrap lifecycle、空state、pause、cycles reserve、RuntimeBinding、controller/module hashの全postconditionを満たしたschema 3 receiptだけを後続profileの根拠にする。

```sh
scripts/production-release.sh deploy --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --canister-install-receipt evidence/production-canister-install.json \
  --receipt evidence/release-id/gate-a-receipt.json -- scripts/production-deploy-driver.sh
```

Gate AとBaseのpause配置まではprofileにも同じBootstrap運用値を要求する。初回Governance operation IDは、Gate Aが証明するfresh・emptyなBootstrap stateのcounter既定値から0を導出する。OperationalConfigSealedまではprepare経路が到達不能であり、通常upgrade receiptがstable public state continuityを固定する。sealは送信前、await後、stable commit内でlive counterが0のままか再検証し、不一致ならBootstrapを維持してfail closedにする。配置後は、このIDとdeployment instanceから導出したsalt、Governance Operator sender、Timelock target、value 0、Bridgeの2つのunpause payload、zero predecessor、24時間delayから再構成できるexact `scheduleBatch` / `executeBatch` calldataのgas estimate、10件以上の異なるFinalized fee block、idle cycles burnを`initial-operational-parameters.json`へ記録する。Gate B profileでは固定式から導出したgovernance EVM fee 8項目、`cycles_floor`、`settlement_cycle_ceiling`だけを置換し、それ以外のGate A profile driftを拒否する。

配置後はproduction installerを単独controllerとして残したまま、Canisterをpause状態で運用設定を一度だけsealする。pre-seal Gate BはGate A lineage、proof、`initial-operational-parameters.json`と導出値を構造検証し、sealだけを認可する。seal後、schedule/executeのprepare wrapperが固定confirmation relayerでFinalized attestationをrefreshし、pause、reserve、live module hash、installer単独controllerを含むfresh live Gate Bを通過した場合だけproduction controllerのprepareへ進む。固定artifactを匿名relay、固定confirmation relayerがconfirmする。24時間後もprepare wrapper内でfresh live Gate Bとcontroller schedule receiptを検証してから、同じ三段階で固定`execute_activation`を実行する。confirm成功だけでは完了扱いにせず、`verify-controller-activation`がFinalized Base結果とCanister状態を束縛したreceiptを発行するまでpauseを維持する。初回executeの確定と同じstable transactionでbootstrap activation認可だけを永久に消費し、production installer自体はユーザーが別途判断するまでcontrollerとして残す。以後は緊急pauseしてもbootstrap認可は復活せず、activationは既存Governance principalだけがprepareできる。SNS custom functionは初回activationに使用しない。7日計測、keeper drill、monitoring receiptはunpause後のGate Cで行う。SNS Root単独controllerへのhandoverとSNS同一Wasm upgradeは初回activationやGate Cから独立し、ユーザーが時期を別途判断した場合だけ実行する。任意のunpause commandは受け付けない。

`initial-operational-parameters.json`の`governance_operation_id`は初回固定値`0`であり、driverがseal送信前に検証する。`seal_operational_config`の公開引数は`OperationalConfigArgs`一つだけとし、Canisterは内部固定値`0`をawait前、await後、stable commit内で次のstable governance operation IDと照合する。不一致ならsealせずBootstrapを維持する。

Gate B前にUIを先行公開する場合は、clean sourceから`production-assets.mjs generate`でasset receiptを作り、review済みGate A release inputsのpre-activation profileを使って`deploy:preactivation:check`を通した後、承認済みの同一入力で`deploy:preactivation`する。このprofileはGate B hash未設定かつdeployment block 0なので、全writeはfail closedになる。Gate B合格後は、検証済みbundleからrenderしたGate-B-bound profileと同じasset receiptを通常production deployへ渡して差し替える。

```sh
COMMON=(--phase schedule --bundle evidence/release-id \
  --release-inputs deployments/generated/release-id \
  --receipt evidence/release-id/gate-a-receipt.json \
  --operational-config-seal-receipt evidence/operational-config-seal-receipt.json \
  --confirm-asset-acceptance SCHEDULE_PRODUCTION_ASSET_ACTIVATION)
ARTIFACT=evidence/activation/schedule-artifact.json

scripts/production-release.sh activate "${COMMON[@]}" --step prepare \
  --artifact "$ARTIFACT" --controller-pem /secure/controller.pem \
  --confirmation-relayer-identity confirmation-relayer \
  -- scripts/production-activate-driver.sh
BASE_RPC_URL=https://reviewed-base-rpc.example \
  scripts/production-release.sh activate "${COMMON[@]}" --step relay \
  --artifact "$ARTIFACT" -- scripts/production-activate-driver.sh
scripts/production-release.sh activate "${COMMON[@]}" --step confirm \
  --artifact "$ARTIFACT" --confirmation-relayer-pem /secure/confirmation-relayer.pem \
  --confirmation-receipt evidence/activation/schedule-confirmation.json \
  --activation-receipt evidence/activation/schedule-receipt.json \
  -- scripts/production-activate-driver.sh
```

24時間後の`execute`はfresh Gate Bを要求し、全stepで`--prior-schedule-receipt`と`UNPAUSE_PRODUCTION_ASSET_ACCEPTANCE`を必須とする。release wrapperはprepare前に`verify-controller-schedule-receipt-live`を実行し、receipt内部digest、installer単独controller、module hash、canonical Finalized Base Timelock pending状態が一致しなければ停止する。confirm後も`verify-controller-activation execute`がcontroller activation receiptを発行するまで資産受付開始を完了扱いにしない。pending transactionのfee replacementはrelay前に`--step replace`をproduction controllerで実行し、元artifact、authorization、binding、profileの回数・fee上限へ束縛した新しいartifactを使用する。署名前に停止してCanisterの`Prepared`だけが残った場合、5分のauthorization期限内なら同じprepareを冪等再開する。期限切れなら古いauthorizationを再利用せず、fresh live Gate Bから新しいauthorization artifactを耐久化してから同じstable operationを再開する。

bundle欠落、test profile、source/profile drift、Gate失敗では後続コマンドを起動しない。Gate Aのdeployコマンドにunpauseまたはresume操作を混在させることも拒否する。
Gate A profileの`deployment_block`は未配置を示す`0`に固定する。deploy前にwrapperはCanister install receiptをtyped profileとclean sourceへ照合する。Base transaction送信直前には、そのreceiptもpredeploy verifierへ渡し、certified `read_state`のmodule hashがreceiptとprofileのWasm SHA-256の両方へ一致し、controller集合がreceiptのinstaller principal単独であることを再確認する。deploy後、wrapperは実receipt blockを入れた`<receipt>.post-deploy-profile.json`を生成し、そのSHA-256とinstall receipt全体をschema 2 Gate A receiptへ固定する。Gate Bはこのpost-deploy profileだけを使う別のlive manifestとし、`parent_gate_a_manifest_sha256`がreceiptのGate A hashと一致し、source/code binding、post-deploy profile hash、実deployment block、install時のCanister identity/module/runtime/pauseが一致しなければならない。さらにGate B profileの`deployment_block`だけを0へ戻したcanonical hashがreceiptのGate A profile hashと一致する必要があり、他fieldの変更は拒否される。staging monitor drillの直接RPC検証は`production-live-preflight.sh verify-monitor-drill BUNDLE`だけを使い、本番Base状態の正本にはしない。
外部`--receipt`はGate B bundle内の`gate-a-receipt.json`とbyte単位で一致しなければならない。

初回contract deployだけ一時EOAを使用し、deployerへroleを残さない。以後のBase管理操作はBridge Canisterがrole別derivationで導出するGovernance Operator、Runtime Administrator、Independent Cancellerから送信する。production IC操作は`BRIDGE_ICP_IDENTITY`とICP CLIへ統一し、`dfx`を使用しない。Timelockの初回activationは、seal時に固定したproduction controllerによるprepare、匿名relay、固定confirmation relayer confirmのschedule/execute二段階とし、各段階でlive preflightを再実行する。初回execute確定後はcontroller集合を変更しなくてもbootstrap activation認可が永久に消費され、以後のactivationは既存Governance principal認可だけを使う。失敗または曖昧結果ではIC/Base pauseを維持し、同じsigned transactionを追跡する。

production CanisterはGate A確定前にpause状態でinstallし、固有のMint SignerとGovernance Operatorをprofileへ固定する。Gate A deploy driverは外部指定のconstructor JSONを使用せず、固定sourceからbuildした`bridge-profile`でbundle内profileを一時directoryへ再生成し、稼働中Canisterの2 addressとpause状態を照合してからcontract deploymentへ渡す。Canisterの再installやdeployment binding APIは実行しない。

profileはCanisterから導出してBaseのFinalized attestationと照合するMint SignerとGovernance Operator、current stable schema、公式EVM RPC Canister ID、単一emergency pause principal、Wasm/bytecode hash、Timelock、固定limit、fee/liveness/reserve関係を含む。Timelock delayはprofileとlive stateの完全一致を要求する。`timelock.runtime_code_hash`は`0x`付き32-byte Keccak runtime code hashであり、生成されたBridge constructor引数、配置直後の実code hash、Gate B Finalized attestationの三者が一致しなければならない。配置後にGate A receiptがBridge/Timelockのcanonical deployment transaction・blockを記録し、Gate Bは公式EVM RPC Canister経由でcurrent runtimeとroleを再照合する。監視欄は通知routingのSHA-256と、検知5分、担当確認15分、Base/IC双方pause 60分のSLOを正確に記録する。

Gate Aはpre-deploy profileとBridge/BSNSの5 build artifact、合計6 artifactを束縛する。Canister install receiptは7番目のartifactへ追加せず、schema 2 Gate A receipt内へ完全に埋め込み、Gate Bへ推移的に継承する。Gate Bはcurrent releaseの6 build artifactに、初期運用値、provider independence、UI、Gate A receipt、不変Gate A profile、production controller upgrade receipt、post-Gate-A policy transitionを加えた正確に13 artifactである。provider independenceはSNS Motionではなく、release source/profile/current Wasm、公式EVM RPC Canister、`BaseMainnet`既定pool、空custom URL、runtime固定3-provider/2-thresholdをschema 2 receiptへ束縛する。既定provider registryと各upstream chainは外部仮定として残す。Gate AでinstallしたWasmとcontroller-bootstrap Wasmが異なる場合は、typed upgrade receiptがsole controller、通常upgrade、前後module、schema、pause、storage/public-state continuityを証明し、policy transitionがそのreceipt hashを固定する。RPC rehearsal、monitor drill、keeper drill、monitoring receipt、7日計測はGate Bに含めず、稼働後のGate Cで要求するが、controller handoverの認可入力にはしない。controller handoverとSNS upgradeもGate Bには含めず、Gate Cが実施時期を決定するものではない。release approver署名と鍵ceremonyは使用しない。Mint Signerはprofile、認証済みCanister公開設定、freshなFinalized Base attestationの三者一致で検証する。x402はBridgeの配置・activation条件ではない。

`validate-bundle --offline`はGate Aの正式なoffline認可判定として`gate_a=pass authorizing=true`だけを成功出力する。`verify-live`はGate Bの構造に加え、5分以内のactivation attestation、公開RuntimeBinding、reserve、production installer identity単独controller、live module hashを認証済みCanister応答で照合する。schedule/execute receiptの検証も初回activationでは同じcontroller条件を使用する。SNS Root単独controllerと同一Wasm SNS upgradeは、ユーザーが時期を別途判断した場合の独立したhandover検証へ分離する。権限principal、rate/cycles policy、Governance fee、固定Ledger feeは、公開RuntimeBindingの`operational_config_sha256`をrelease profileから再構成した値と照合する。実値の確認はcontroller/governance限定`get_operational_config`を使う。認証またはpostconditionが欠ければ非ゼロ終了する。

credential、seed、private key、hardware wallet backup、credential入りRPC URLはprofileやevidenceへ記録しない。

## IC mainnet × Base Sepolia test staging

Plan 007のIC stagingは、現在の`sepolia-staging` bindingに固定された`bridge-sepolia`（`rlhjx-iyaaa-aaaaf-qcnyq-cai`）、deployment instance、Base contracts、signer、共有`testicrc` Ledger/Indexを維持する。2026-08-27/28のreinstallとfresh-stack作成は一度限りの履歴であり、再実行またはresumeしない。今後のCanister更新はschema v35／wire v30を保つsame-instance `upgrade`だけを許可する。test frontendはIC Asset Canisterへ配置せず、静的assetをCloudflare Worker `kinic-bridge-ui-test`から配信する。KINIC Ledger、Base Mainnet、SNSには触れない。

外部配置前にリポジトリ直下の`scripts/plan007-local-gate.sh /secure/work/local-e2e.json`をclean commitで実行し、repo外へ証跡を発行する。dirty treeまたはhash driftでは証跡を発行しない。外部deploy、cycles投入、Base Sepolia transaction、Cloudflare Worker公開はそれぞれ別の明示承認後に行う。

外部stageはschema v8だけを`scripts/plan007/staging-e2e-driver.sh`で新規初期化し、`bootstrap_attestation → preflight → current_schema_upgrade → post_upgrade_binding → frontend_publish → smoke_e2e → wallet_e2e → refund_rehearsal → rpc_rehearsal → live_acceptance`の順で記録する。v7証跡は読取専用履歴であり、resume、migration、dual-read、現行合格判定に使わない。RPC rehearsalのpause後は`live_acceptance`が別operationのreactivationとunpaused postconditionを検証し、全条件を満たしたv8だけを`SHORT_DELAY_LIVE`とする。異なるinstance、reinstall、旧・未知schema、未登録module／Candidの組はfail closedにする。

既存staging Bridge Canister IDは`.icp/data/mappings/sepolia-staging.ids.json`の`bridge-sepolia`を正本とし、新しいmappingを作らない。既存`testicrc`を新規作成対象としてmappingへ追加しない。frontendは`deployments/sepolia-staging/frontend-profile.json`が完成するまでbuildまたは公開せず、完成後に`ui`のstaging artifact driverでCloudflare Worker `kinic-bridge-ui-test`へ公開する。test frontendはBase Mainnet、production Canister ID、非公式EVM RPC Canister IDを拒否し、TEST bannerを常時表示する。

## ICP mainnet上のBase Sepolia staging Bridge deploy先

deploy先は既存`rlhjx-iyaaa-aaaaf-qcnyq-cai`とし、`.icp/data/mappings/sepolia-staging.ids.json`の`bridge-sepolia`をそのまま使う。既存deployment instance、minimum Withdrawal ID、Base contract binding、schema v35／wire v30を保つ`upgrade`だけを許可し、`install`、`reinstall`、`auto`を拒否する。将来reinstall用のinit templateやrender/validate commandは提供しない。

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
