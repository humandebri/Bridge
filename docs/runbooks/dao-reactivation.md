# 個人controller保持下でのDAO再開実証

この手順は本番公開後の再開経路を検証する。初回activationとcontroller移譲を自動実行しない。個人はコード更新と現在の緊急停止を保持し、通常管理操作を個人に追加しない。

承認済みのproduction baselineは[production checkpoint](../../deployments/checkpoints/README.md)に固定したschema 36、module SHA-256 `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`である。これは旧terminal `6192841b...`から`d48d4737...`と`bf047794...`への署名済みupgrade receiptを検証してrotationしたcheckpointであり、source revision `16cd903af92878ffe13294cf1dc577550ba1340c`の保存Wasmと再現buildがlive moduleへ一致する。

## 開始時点

2026-09-16の再確認ではBridge `lb5i5-ziaaa-aaaar-qcgwq-cai` はschema 36、module SHA-256 `bf0477947b06a06d31aa32b52992a1b775fca0c9b4c98bf3129037d47abedd64`、controllerはproduction identity `lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe`一件だった。Governanceは`74ncn-fqaaa-aaaaq-aaasa-cai`、pause principalはproduction identityで、BridgeはSNS Rootの`dapps`に未登録である。開始時点の[baseline manifest](../evidence/dao-baseline-20260914/manifest.json)はrotation前の読み取り記録として保持する。

controller移譲を開始する直前にもlive stateを再取得し、active checkpointの`bf047794...`またはその後に承認したcurrent-source suffixへ完全一致させる。不一致を生観測だけで置換しない。現行`bf047794...`は`storage_integrity_check`を提供するが、handover completionが要求する`get_release_storage_integrity`を提供しないため、このrunbookの`prepare`より前に同methodを含むcurrent-source v36 upgradeを別途検証・承認・実行し、そのreceiptをactive checkpoint evidenceのsuffixへ追加する。

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

実証結果を確認して運用者が別途承認するまでは、Root追加・管理対象登録・個人controller削除を実施しない。承認後も一度にRoot単独へ置換しない。handover driverの`prepare`でRootだけを追加し、controller集合が正確にproduction identityとRootの二者であることを確認してschema 5のpreparation receiptを保存する。結果が不明なら`recover`で再取得し、管理操作は再送しない。

共同controllerを確認した後、`production-handover-registration-proposal.sh`でレビュー済みの標準`RegisterDappCanisters` proposalを一度だけ提出する。提出失敗または実行失敗では個人controllerを保持し、自動再送・自動削除を行わない。proposalのexecutedを確認した後、handover driverの`complete`でGovernanceのproposal、Rootの`dapps`、Root単独controller、module・runtime・storageの継続を検証し、schema 5のcompletion receiptを別ファイルへ保存する。Root単独だが未登録、登録済みだが個人controllerが残る、第三controllerがある状態はすべてインシデントとして停止する。

登録提出scriptにもseal／schedule／execute receiptを環境変数で渡す。scriptは提出前に既存のtyped handover validatorとproof gateを再実行し、review済みpayloadがBridge一件だけの固定actionと完全一致する場合だけjournalを予約して送信する。

testflightを中止する場合は、controller集合が正確にproduction identityとRootの二者で、登録proposalが未実行であることを再確認し、別途承認した操作でRootを削除する。完了後はUpgradeSnsControlledCanisterで同一非圧縮Wasm、mode=3（upgrade）、空Candid引数を指定し、proposal成功と保存状態・runtime・入出金・UI検証を確認する。

移譲用提出物は次で生成する（送信は行わない）。ROOT_RESPONSE_JSONはRootの`list_sns_canisters (record {})`の最新の`--json`応答、EXPECTED_SHA256は検証済みの現在の非圧縮module hashを指定する。

```sh
node tools/sns-proposal/handover.mjs ROOT_RESPONSE_JSON BRIDGE WASM EXPECTED_SHA256 OUTPUT_JSON
```

出力には登録済み判定、登録proposal、同一Wasmの標準upgrade proposal、1MBごとのchunkファイルと各SHA-256を含む。移管開始時にBridgeが既にRootへ登録済みなら状態不整合として停止し、再登録を省略して続行しない。BridgeのWasmはingress上限を超えるため、別途承認した移譲準備で個人controllerがBridge自身のchunk storeへuploadし、返却された各hashと`stored_chunks`を読み戻す。uploadはRoot追加前に完了させる。proposalはその順序付きhash一覧と元の非圧縮Wasm hashを固定する。圧縮によってmodule hashを変えない。chunked upgradeもSNSの標準経路を使用する。[公式SNS管理手順](https://docs.internetcomputer.org/guides/governance/managing/)

緊急停止principalは保持する。移譲後の障害を個人identityで直接修復できるとは扱わない。

## ローカル検証

`scripts/prepare-sns-test-runtime.sh`は公式リリース`release-2026-09-10_03-28--all-in-one-node`のハッシュ固定アーカイブからGovernance／Rootを展開する。`BRIDGE_SNS_TEST_RUNTIME`で配置先を指定できる。rootはtestflight=falseで起動する。

`integration/phase3.spec.ts`の`real SNS reactivation and production registration`は実Governanceによる登録・採択・実行、Bridgeの再開、個人controller維持、Root登録による個人controller削除、同一Wasm upgradeを検証する。Base RPCとLedgerは既存mockを使用するため、本番の24時間待機や実資産の確認を代替しない。

ローカルの早期executeは署名準備まで成功し得る。24時間の強制はBaseのTimelockが担うため、SNSのexecutedだけで再開と判断しない。実SNSテストはmockのrevertを通知して停止が継続することを確認し、Timelockそのものの時間制約は既存のSolidityテストで検証する。

checkpointを指定した移譲driverは、初回Gate B・seal・controller activationの履歴をcheckpoint rootsと照合したうえで、現在のclean sourceがterminalのsourceと一致すること、完全proof、2回の再ビルドによる同一module hashを要求する。現在の個人単独controllerと、移譲後のRoot単独controllerは別々に検証する。

Rootはupgrade要求への応答後に実際の更新を実行するため、proposalのexecutedと同一module hashだけでは更新完了としない。`get_release_upgrade_observation`から、採択・移譲以降にSNS Rootが成功させた`post_upgrade`の完了時刻とcallerを確認する。この記録はheapのみで保持し、stable schemaは変更しない。失敗したupgradeは完了記録を更新しない。同時に別のupgradeを実行しないことを運用上の前提とし、検証中に別proposalによる更新があれば証跡を取り直す。ローカル実SNSテストでは、Rootが応答した後にpost_upgradeがtrapするケースと、その後の正常な同一Wasm更新を区別する。
