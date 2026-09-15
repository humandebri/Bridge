# 個人controller保持下でのDAO再開実証

この手順は本番公開後の再開経路を検証する。初回activationとcontroller移譲を自動実行しない。個人はコード更新と現在の緊急停止を保持し、通常管理操作を個人に追加しない。

承認済みのproduction baselineは[production checkpoint](../../deployments/checkpoints/README.md)に固定したschema 36、module SHA-256 `6192841b3c2b5c28c6decea307e7c8e23700ab9bb00e6e593d74857478563c75`である。開始時点の生応答とSHA-256は[baseline manifest](../evidence/dao-baseline-20260914/manifest.json)に監査用の観測として固定しているが、checkpointまたはcutoverの承認を代替しない。

## 開始時点

2026-09-14の読み取りではBridge `lb5i5-ziaaa-aaaar-qcgwq-cai` はschema 36、controllerはproduction identity `lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe`一件だった。Governanceは`74ncn-fqaaa-aaaaq-aaasa-cai`、pause principalはproduction identityである。一方、生応答のmodule SHA-256 `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`は、承認済みcheckpointの`6192841b3c2b5c28c6decea307e7c8e23700ab9bb00e6e593d74857478563c75`と一致しない。この生応答は未承認の差分を示す監査証跡としてのみ保持する。

この不一致がある間はfail closedとし、本runbookのproposal準備・再開実証・controller移譲を開始しない。`bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`を現行terminalとして扱うには、そのupgrade chainを検証した承認済みcheckpointまたはcutover記録を先に追加する。そうでなければlive stateを再取得し、承認済み`6192841b3c2b5c28c6decea307e7c8e23700ab9bb00e6e593d74857478563c75`との一致を確認する。生観測だけを根拠に承認済みhashを置換しない。

## 準備

1. 現在のclean sourceで対象proofとテストを完了し、承認対象のWasm・source revision・公開Candidを固定する。本番upgradeは別途承認後に実行する。
2. Governanceの`list_nervous_system_functions`をqueryし、`icp --json`の`response_bytes`を含む応答を保存する。Rootの`list_sns_canisters (record {})`、Bridgeのmanagement status、runtime、lifecycle、activation、運用設定も保存する。
3. `tools/sns-proposal/prepare.mjs REGISTRY_JSON BRIDGE PREVIOUS_OPERATION OUTPUT_JSON`を固定Nodeで実行する。PREVIOUS_OPERATIONは`get_activation_status`の直前の確定operation IDである。出力は送信しない。active／reserved IDを避けて登録proposalを生成し、既存登録はtarget・validatorの完全一致を要求する。
4. 2操作それぞれのfunction ID、target、validator、method、topic、payload、提出neuron・signerとproposal全文をレビューする。登録proposalが必要ならその提出を別途承認し、executedとlive registryを確認する。登録だけではBridgeは再開しない。

`prepare.mjs`が出す2操作のpayloadは、同じ取得時点のoperation IDを使用する。scheduleの確定後は新しいIDで再生成する。先に作ったexecute payloadを送らない。

専用APIは`validate_sns_schedule_activation`／`sns_schedule_activation`と`validate_sns_execute_activation`／`sns_execute_activation`。引数はいずれも`record { previous_governance_operation_id : nat64 }`。validatorは`Result<text,text>`を返す読み取り専用updateで、実行入口は失敗をIC rejectとして返す。SNSのexecutedは署名準備の完了までを意味し、Baseの完了ではない。

## 本番実証（各操作の別途承認後）

- 停止日時と少額テスト額を実行用のproposal・作業記録へ明示する。現在のemergency pause principalで停止し、Base両flowとIC Depositの停止を確認する。
- 最新registryと確定operationでschedule proposalを準備・提出する。既存の`production-activation-proposal.sh`には末尾にPREVIOUS_OPERATIONとレビュー済みproposal準備JSONのpathを渡す。送信前に最新registryから再生成し、function ID・validator・payload・proposal本文の一致を検証する。checkpointのある送信は再送せず、`OUTPUT.response.json`に保存したCLI応答とproposal履歴を読み戻す。応答のデコードに失敗してもjournalを削除しない。
- signed transactionをrelayし、指定confirmation relayerによる通知とCanisterのFinalized検証を完了する。revertなら停止し、確定待ちのタイムアウト扱いにしない。
- 24時間後にlive状態・registryを再取得し、確定したschedule operation IDに束縛したexecute proposalを別途準備・承認・提出する。relayとFinalized検証まで完了し、Base両flowとIC Depositの再開、少額入出金、データ・準備金・auditの継続を確認する。
- SNS submissionはschema 4、activation receiptはschema 5。旧SNS形式を変換して通さない。初回controller receiptは別型のまま保存する。
- 手数料・受取先変更はこの本番実証には含めない。

再開receiptを生成する`bridge-profile verify-activation`と`verify-schedule-receipt-live`には、`BRIDGE_CHECKPOINT_EVIDENCE`で承認済みの現在のv36 terminalを指定する。初回Gate Bを履歴として検証し、現在のmoduleだけをcheckpointから導出する。Gate B単独controller条件は変わらない。

## 最終移譲

実証結果を確認して運用者が別途承認するまでは、Root追加・管理対象登録・個人controller削除を実施しない。承認後に`BRIDGE_CHECKPOINT_EVIDENCE`で現在の承認済みterminalを固定し、handover driverでRoot単独へ変更し、標準RegisterDappCanistersの実行を確認する。既に管理対象なら再登録しない。続くUpgradeSnsControlledCanisterで同一非圧縮Wasm、mode=3（upgrade）、空Candid引数を指定し、proposal成功と保存状態・runtime・入出金・UI検証を確認する。

移譲用提出物は次で生成する（送信は行わない）。ROOT_RESPONSE_JSONはRootの`list_sns_canisters (record {})`の最新の`--json`応答、EXPECTED_SHA256は検証済みの現在の非圧縮module hashを指定する。

```sh
node tools/sns-proposal/handover.mjs ROOT_RESPONSE_JSON BRIDGE WASM EXPECTED_SHA256 OUTPUT_JSON
```

出力には登録済み判定、登録proposal（登録済みならnull）、同一Wasmの標準upgrade proposal、1MBごとのchunkファイルと各SHA-256を含む。BridgeのWasmはingress上限を超えるため、別途承認した移譲準備で個人controllerがBridge自身のchunk storeへuploadし、返却された各hashと`stored_chunks`を読み戻す。uploadはcontroller移譲前に完了させる。proposalはその順序付きhash一覧と元の非圧縮Wasm hashを固定する。圧縮によってmodule hashを変えない。chunked upgradeもSNSの標準経路を使用する。[公式SNS管理手順](https://docs.internetcomputer.org/guides/governance/managing/)

緊急停止principalは保持する。移譲後の障害を個人identityで直接修復できるとは扱わない。

## ローカル検証

`scripts/prepare-sns-test-runtime.sh`は公式リリース`release-2026-09-10_03-28--all-in-one-node`のハッシュ固定アーカイブからGovernance／Rootを展開する。`BRIDGE_SNS_TEST_RUNTIME`で配置先を指定できる。rootはtestflight=falseで起動する。

`integration/phase3.spec.ts`の`real SNS reactivation and production registration`は実Governanceによる登録・採択・実行、Bridgeの再開、個人controller維持、Root登録による個人controller削除、同一Wasm upgradeを検証する。Base RPCとLedgerは既存mockを使用するため、本番の24時間待機や実資産の確認を代替しない。

ローカルの早期executeは署名準備まで成功し得る。24時間の強制はBaseのTimelockが担うため、SNSのexecutedだけで再開と判断しない。実SNSテストはmockのrevertを通知して停止が継続することを確認し、Timelockそのものの時間制約は既存のSolidityテストで検証する。

checkpointを指定した移譲driverは、初回Gate B・seal・controller activationの履歴をcheckpoint rootsと照合したうえで、現在のclean sourceがterminalのsourceと一致すること、完全proof、2回の再ビルドによる同一module hashを要求する。現在の個人単独controllerと、移譲後のRoot単独controllerは別々に検証する。

Rootはupgrade要求への応答後に実際の更新を実行するため、proposalのexecutedと同一module hashだけでは更新完了としない。`get_release_upgrade_observation`から、採択・移譲以降にSNS Rootが成功させた`post_upgrade`の完了時刻とcallerを確認する。この記録はheapのみで保持し、stable schemaは変更しない。失敗したupgradeは完了記録を更新しない。同時に別のupgradeを実行しないことを運用上の前提とし、検証中に別proposalによる更新があれば証跡を取り直す。ローカル実SNSテストでは、Rootが応答した後にpost_upgradeがtrapするケースと、その後の正常な同一Wasm更新を区別する。
