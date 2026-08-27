# Bridge資源補充・緊急停止

## Schema v35 baseline

初回mainnet deployで導入するv35は、capacity reservation、`nonterminal_deposit_owner_index`、役割別governance nonce lane、indexed funding recovery deadline、認可発行時刻を含む現在のSQLite形状だけを正本とする。旧・未知schema、tableやcounterが欠落したDBはreopen時にfail closedとなる。初回mainnet deploy完了後はこの形状をproduction baselineとして固定し、以後の形状変更はschema番号を上げた明示migrationとして扱う。

## 日常確認

- Bridgeのcycles、Governance Operator ETH、governance reserve surplus、Finalized観測時刻、Governance pending nonce、停止理由を確認する。さらに未決済Authorization、未決済Withdrawal件数・`amountOut`合計・最古観測時刻・Ledger停止理由を確認する。Mint Signer ETHは要件ではない。
- Fee Recipient、RPC credential、raw transaction、秘密情報を監視ログへ出さない。
- `get_bridge_status.counts`の`reserved_deposit_mint_operations`、`reserved_deposit_mint_amount`、`pending_ledger_operations`、`retained_audit_events`、`pruned_audit_events`、`retained_deposit_index_entries`と`audit_retention_warning`を確認する。audit詳細は直近100,000件で、80,000件以上は警告する。通常Deposit一覧はownerごとに直近100件だが、非終端Depositは独立indexから全件をpagination取得できる。

Status画面と資産操作直前のruntime事前検証は、ブラウザからreview済みprofileのBase RPCへ直接問い合わせる観測であり、CanisterのHTTP outcallを発生させない。Bridge画面の通常表示はruntime readinessを自動検証せず、手数料とpauseの軽量Quoteだけを読む。Deposit availabilityは操作直前にBase Finalized/Safe、runtime、pause、epoch、IC pause、Canister cycles floorを組み合わせ、60秒を超えた結果を再利用せずfail closedにする。Mint Signer ETH残高は条件にせず、Governance availabilityだけはGovernance Operator ETH floorを確認する。資産状態を変える最終判断ではブラウザ観測を信用せず、Canisterがprovider quorumでBaseを再検証する。

CanisterがFinalized headを取得する際のblock response上限は固定16 KiBである。上限超過時は応答上限の自動拡大や自動再試行をせず、RPC unavailableとしてLedger処理前にfail closedにする。receipt blockは取得せず、2-of-3で一致したreceipt hashへ4 KiB上限の`bridgeSnapshot()` EIP-1898 probeを実行し、`requireCanonical=true`とsnapshotのblock numberでcanonical receiptを確認する。
本番preflightも、receipt、deployment、保存snapshot、Timelock role eventの既知block hashをBridgeの`bridgeSnapshot()`またはTimelockの`getMinDelay()`へEIP-1898で固定する。番号指定block取得は行わず、full block応答はFinalized headのhash発見だけに使う。
2026-07-23のBase Sepolia検証では、直近256 Finalized blockの`eth_getBlockByNumber`応答は最大5,542 bytesであり、16 KiB上限内に収まった。

Canisterが使用するLedger feeの単一の定義元は`canister/bridge-canister/src/ledger.rs`の`KINIC_LEDGER_FEE`である。
Canisterの全Ledger処理がこの値を使う。UIはBridge Canisterを経由せず、対象Ledgerの`icrc1_fee()`をqueryして表示と事前検証へ使う。

production Canisterが受け入れるLedger feeは`100000` raw、`test-deployment` featureで作るstaging Canisterは`10000` rawに固定する。activation preflightとruntimeの`BadFee`処理は、buildが選択した固定値との差異をfail closedにする。
詳しい検証条件は`sepolia-staging-e2e.md`の「Test Ledgerのfee」に記載する。

production artifactへstaging Wasmを流用しない。
production buildでは定数をKINIC mainnet Ledgerのlive feeと承認済みprofileへ同期し、Candid binding、Rust/UI/integration test、production preflightを同じ変更で更新する。

stable schemaはv35、record wireはv30を現行形式とする。Productionとtest-deploymentの`post_upgrade`は現行形式だけを受理する。旧stagingは移行またはreinstallせずpaused/read-onlyで保持し、新しいCanisterへ現行形式を初回installする。

## 保持制限と監査

`get_audit_events`で指定したsequenceがpruning済みの場合、応答は`oldest_available_sequence`から始まる。削除済みeventの完全な内容はcanister内に残らず、`pruned_count`、`pruned_through_sequence`、`pruned_digest`だけがcommitmentとして残る。今回は外部archiveや第三者timestampingを行わないため、詳細の長期保管が必要な運用では上限到達前に別系統へ取得する。

`list_deposit_ids.history_truncated = true`はownerの古い一覧索引が削除済みであることを示す。`oldest_available_cursor`より古いDepositでも既知IDによる`get_deposit`と同一requestの冪等retryは利用できる。

Productionではschema v35またはwire v30以外のstable state、未知schema、decode不能なDBを、空であってもfail closedで拒否する。

`get_bridge_status.withdrawal_fee_guard_active`がtrueになった場合は、Base Bridgeのwithdrawalを直ちにpauseする。該当recordの`last_settlement_stop_reason`と監査eventに`LedgerFeeExceedsServiceFee`が残り、IC releaseやreserve変更は行われない。buildが選択した固定`KINIC_LEDGER_FEE`（productionは`100000 raw`、stagingは`10000 raw`）とprepared recordのcharged Service Feeをreview済みprofileに照合した後、任意の非anonymous主体がHistoryから`continue_withdrawal`を実行する。Canisterはruntimeで`icrc1_fee()`を照会せず、固定Ledger Feeがcharged Service Fee以下であることを再検証できた場合だけ、同じrecordからreleaseを開始してguardを解除する。
現行形式はstable schema v35／record wire v30とし、これ以外をfail closedで拒否する。staging replacement policyは旧stackのpause・負債ゼロ証跡、新規target module・Candid hash、immutable設定、confirmation relayer、fresh deployment instanceを固定する。初期化済みの永続Canisterは同一deployment instanceのcurrent-schema upgradeだけで更新し、reinstallは禁止する。
SQLite DBやcounterを手作業で変更しない。

schema versionの正本は`bridge_metadata.application_schema_version`だけである。Depositはrecord、owner sequence、Base recipient、Authorization、失効またはMint確定証拠を一つのstable envelopeへ保存する。pending Ledger、open reconciliation hold、nonterminal Withdrawalの件数は各indexの`table_counts`を正本とし、primary rowとliability index・集計は一つのSQLite transactionで更新する。

## Controller限定のstorage保守

保守APIはCanister controllerだけが呼び出せる。Governance principal、pause principal、Runtime Administratorには許可しない。実行前に対象Wasmとstable imageを保存し、処理失敗時にmetadataやDBを手作業で変更しない。

新規install後、controller handover前にcontroller identityから`initialize_public_config()`を一度実行する。これはMint SignerとGovernance Operatorをchain-keyから導出し、両方が成功した場合だけstable stateへ一括保存する。続けて公開`get_runtime_binding()`とcontroller/governance限定`get_operational_config()`を実行し、release profileの両アドレスと一致することを確認する。`get_runtime_binding().operational_config_sha256`は、operational実値と固定Ledger feeをdomain-separated Candid encodingで束縛し、handover後の`verify-live`がprofile driftを公開値なしで検出する。初期化失敗、未初期化query、保存済みアドレスまたはbinding digestの不一致はいずれもdeployment blockerであり、空値や手入力値で代替しない。upgradeでは保存値を維持し、再初期化を必須としない。

1. `start_storage_validation()`を一度呼び、`continue_storage_validation(100)`を`complete = true`まで反復する。途中で通常更新が入って`StateChanged`になった場合、古いprogressは破棄されるため、静穏時間帯に明示的に再開始する。
2. `storage_integrity_check()` queryが`ok`を返すことを確認する。upgrade処理からこの検査は自動実行されない。
3. `refresh_storage_checksum(4194304)`を`complete = true`まで反復する。一回の呼出しは最大4 MiBであり、raw stable-memoryコピーやfilesystem backupとして扱わない。

初回mainnet deployまではschema変更を現行形式へ直接反映し、全caller・test・fixture・文書を同時更新する。WAL、mmap、compaction、旧`ic-sqlite-vfs` layoutからの移行、dual-read、fallbackは行わない。

## ETH・cycles補充

- ETHはGovernance Operator addressだけへ補充する。Governance transactionは署名前にSafe/Finalized残高の小さい方がそのtransactionの最大費用以上であることを検査する。Mint Signerへ補充せず、Deposit admissionやAuthorization発行にETHを要求しない。SNS-token feeの自動交換は行わない。
- cyclesはBridgeの30日floorとfreezing thresholdの両方を満たすことを確認する。
- permissionlessな`request_deposit`は、cycle reserveと既定60秒windowのglobal 30件・principalあたり3件のquotaを通過してから有料Base preflightを開始する。開始を許可されたattemptはBase検証またはLedger fundingが後で確定失敗してもquotaを消費し、attemptとactive reservationの削除でquotaは戻らない。`RateLimited`と`ReserveUnavailable`の増加を監視し、window reset前に無資金requestを反復しない。
- permissionlessな`notify_withdrawal`は、既定では600秒あたりglobal 60件まで有料EVM RPCを開始でき、canonical confirmed eventの永続化は別のingestion上限30件で制限する。missing、pending、reverted、不正response、duplicateはingestion枠を消費しない。verification上限は無権限trafficによる消費速度を制限するが正当通知用の枠を予約しないため、Sybil trafficで枯渇したwindowでは正規通知も一時的に遅延し得る。`RateLimited`の増加とcycles減少を監視し、verification上限を最悪時の許容RPC予算以下に設定する。枠枯渇時は追加通知を反復せず、攻撃またはprovider障害としてpause判断を行う。
- 補充後も自動resumeしない。Governanceが観測回復と資産状態を確認してからBridgeをresumeする。

## EVM RPC provider

本番Canisterのoutcallは公式EVM RPC Canisterの組み込み`BaseMainnet`だけを使い、`custom_evm_rpc_urls`は空配列とする。初回配置時の単一`BASE_RPC_URL`はEOA transaction送信transportに限定し、production profile、release bundle、UI、evidenceへ保存しない。Gate A/Bのchain binding、Finalized receipt、runtime、role、pause確認はCanisterが公式経路から取得した監査記録を正本とする。Base Sepolia stagingと故障演習のcustom providerは別設定として扱う。詳細は[ADR 0024](../adr/0024-validate-rpc-chain-binding-before-runtime.md)と[ADR 0027](../adr/0027-use-dedicated-eoa-for-initial-base-deployment.md)を正本とする。

## 緊急pause

監視目標は同一の障害起点から5分以内の検知、15分以内の担当者確認、60分以内のBaseとIC双方のpauseである。片側だけのpauseで完了扱いにしない。単一emergency pause principalの実request IDとaudit event、両transaction/callの確定時刻をevidenceへ入れる。本番ゲートはpause/cancel経路の成功、証跡、公式EVM RPC Canister IDとchain、pending Timelock cancel可能性を要求し、5/15/60の実測達成は公開後に評価する。EVM RPC Canister配下providerの運営主体・基盤・可用性は外部仮定として扱う。

- 承認済みpause principal identityから`pause_new_deposits`を実行する。未期限Mint AuthorizationはBase側の`pauseDepositMints`によるepoch増加で失効するが、返金は元deadline後のFinalized未処理証拠まで待つ。各Authorizationのdeadlineと停止理由を監視する。
- Base側の異常では単一emergency pause principalがCanisterの`emergency_pause`を呼ぶ。この呼出しの成功条件はIC側pauseとBase action queueの永続化までであり、Baseへの送信完了ではない。同じpause principal identityを使う外部CLIで`drain-emergency`を実行し、Deposit/Withdrawal pauseと記録済みTimelock cancelを順に署名・送信・確定する。
- 緊急pauseと競合した、まだ署名成果物を発行していないfee・schedule・execute操作だけを破棄して未使用nonceを再利用できる。`SignedAwaitingRelay`は外部送信済みの可能性があるため破棄しない。既送信executeが成功しても緊急actionが残る間はICをresumeせず、Base pauseを優先する。
- 再開ごとの`schedule_activation`はCanisterの単調増加operation IDから新しいTimelock saltを導出する。過去に完了したactivationを再利用せず、保存されたoperation IDとsaltの組だけを`execute_activation`またはcancelへ渡す。
- Governance Operatorのnonce競合時にCanisterは自動回復しない。CLIは発行済みhashを確認し、exact raw transactionの再送または既知transaction応答だけを冪等成功として扱う。別hashによるnonce消費が疑われる場合は停止して監査し、stable nonceを手作業で変更しない。

## Governance relayer

CanisterはBase governance transactionをbroadcastせず、governance timerも持たない。署名済みraw transactionの取得とbroadcastは匿名で行える。`confirm`と`run`はrelease profileへ固定した専用confirmation relayer identityを使い、障害時だけGovernance/Pause principalを復旧callerとして使う。初期配置はroleを残さない外部EOAで行い、Service Feeとactivation schedule/executeの署名要求にはGovernance principal identityを使う。

```bash
export BRIDGE_CANISTER_ID='...'
export IC_IDENTITY_PEM='/secure/path/governance.pem'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
npm run governance-relayer -- prepare --action pause-deposits
# confirmation relayer PEMへ切り替える
export IC_IDENTITY_PEM='/secure/path/confirmation-relayer.pem'
# 手動診断時だけ、専用confirmation relayer identityでBase観測を更新する
npm run governance-relayer -- refresh-attestation
npm run governance-relayer -- run
```

`run`はpending署名成果物の取得、raw transactionのhash・chain・sender・nonce・target・calldata・gas・fee検証、broadcast、Finalized待機、Canister確定通知を行う。reverted receiptを検出した場合はFinalized待機を直ちに止め、確定後に`confirm --operation-id <id> --transaction-hash <hash>`でCanisterを終端化する。broadcast直後に停止した場合は`status`で同じoperationを確認して`run --operation-id <id>`を再実行する。同じraw transactionの再送とRPCの`already known`は冪等成功であり、`nonce too low`はexpected hashのreceiptが存在する場合だけ既送信として扱う。

threshold signingの一時障害でoperationが`Prepared`に残ると、`status`は`SigningUnavailable`を返す。通常操作は同じ`prepare --action ...`、activationは同じCanister API、緊急操作は`drain-emergency`を再実行する。再試行は保存済みnonce、target、calldata、feeを変更せず、署名済み成果物がある場合は再署名しない。

詰まり時のreplacementは自動生成しない。現在のhashと新しいfeeを人間が確認し、次を明示実行する。Canisterは同一operation・nonce・target・calldata・gasを維持し、直前generation比12.5%以上、設定ceiling内、最大3回だけ再署名する。CLIは独自署名やfee変更を行わない。

```bash
npm run governance-relayer -- replace \
  --operation-id <id> \
  --max-fee <wei> \
  --priority-fee <wei>
npm run governance-relayer -- run --operation-id <id>
```

配置後のGovernance relayerは`status`と`relay`を匿名で実行できる。`confirm`とconfirmationを含む`run`は専用confirmation relayer identityを必須とし、障害復旧時だけGovernance/Pause principalを使う。`prepare`、`replace`、activation、緊急操作の明示要求には対応するGovernance/Pause identityを使う。初回配置だけは暗号化Foundry keystoreと別password fileを入力とする`production-deploy-driver.sh`で行う。秘密、実path、RPC URLをrelease artifactやevidenceへ記録しない。

旧stagingの切替前にpending Deposit/Withdrawal、reserve、pending governance transaction、Timelock queueを監査し、Base/IC双方をpauseする。負債または処理中recordがあれば解消まで切替を停止する。旧stackはupgrade・reinstall・破棄せずread-only証跡として保持し、fresh Timelock、Bridge、bSNS、専用signer、deployment instance、IC Bridge Canisterを構築する。rollbackでは新stackをpauseし、旧stackを自動再開しない。

初回production Canister作成は`icp.yaml`へsubnetを設定せず、review済みidentityで`BRIDGE_ICP_IDENTITY=<identity> scripts/production-canister-bootstrap.sh`を実行する。このscriptは`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeq5-fk5o7-yae`を`icp canister create --subnet`へ固定し、作成後または既存mapping再利用時にNNS Registryが返す実subnetとの一致を必須にする。`.icp/data/mappings/production.ids.json`に既存IDがある場合は新規作成しない。

mappingをcommitしたclean sourceで`production-canister-plan.template.json`からrepo外のschema 1 planを作り、同じsourceから再buildしたWasmだけを`scripts/production-canister-install.sh --plan ... --wasm ... --receipt ...`へ渡す。scriptはinstall専用modeとraw Candid binaryを固定し、初期化後のmodule/controller、Bootstrap lifecycle、空state、pause、storage validation/checksum、cycles reserve、RuntimeBindingをtyped receiptへ記録する。途中失敗後は再実行せずlive statusを調査する。receiptから確定したMint SignerとGovernance Operatorを最終release profileへ反映し、Gate A wrapperへ`--canister-install-receipt`として渡す。wrapperは同じ凍結receiptをdeploy driverへ渡し、Base送信直前にcertified `read_state`のmodule hashとinstaller単独controllerを再検証する。Base配置成功後、governance principalからprofileと完全一致する運用設定で`seal_operational_config`を一度だけ実行する。この更新は公式EVM RPC Canisterの`BaseMainnet`観測がruntime、role、pause条件を満たす場合だけ設定とactivation attestationを原子的に保存する。handover driverを直接使う場合は、同じinstall receiptを`BRIDGE_CANISTER_INSTALL_RECEIPT`へ、Base配置完了後にwrapperが出力したschema 2 receiptを`BRIDGE_GATE_A_RECEIPT`へ、`<receipt>.deployment-binding.json`を`BRIDGE_DEPLOYMENT_BINDING_FILE`へ指定する。driverはtyped binding/receiptの一致後、認証済みqueryとcertified `read_state`で`OperationalConfigSealed`、freshかつ両deployment block以後のattestation、install時module/controllerを再検証する。RuntimeBindingはempty RPC digestに固定し、statusが返すTTL/epochとprofileのgovernance EVM fee、cycles floor、settlement cycle ceilingを含む全運用設定から`operational_config_sha256`を再構成する。pause/reserve/全countが空でない場合もcontroller移譲を開始しない。Bootstrap、欠落・古いattestation、profile drift、module/controller driftも同様に拒否する。production profileの`base_rpc_url`は`null`、`rpc_providers`は空配列のままにし、直接Custom RPCをhandoverへ注入しない。

本番資産受付は、Gate Aでoffline artifactとconstructor条件を承認し、Timelock／Bridgeを専用EOAからpause配置する。配置後はSNS Governanceが運用設定を一度だけ封印し、その処理でCanisterの公式EVM RPC監査によるruntime、role、EOA権限ゼロ、pauseを確認してactivation attestationを保存する。handover driverが封印状態と保存済みFinalized attestationを再検証した後にだけcontrollerをSNS Rootへ移譲する。さらに7日間・10件以上のfee／cycles計測、monitor drill、emergency pause、主要5 RPC scenario、reserve確認を完了する。これらを含むGate B承認後だけ`production-release.sh activate --phase schedule --confirmation-relayer-identity <name>`で固定SNS proposalを提出する。driverはproofとartifact再buildを完了してからnamed ICP identityのprincipalをprofileと照合し、`refresh_activation_attestation`、署名付き`verify-live`、source再照合を連続実行する。提出応答だけでは完了扱いにせず、`bridge-profile verify-activation schedule`がSNS実行状態、Canisterのpending operation、Canisterが独立確認したFinalized Base transactionを束縛したschema v4 schedule receiptを発行するまでpauseを維持する。24時間後は古いGate Bを再利用せず、新しいGate Bとschema v4 schedule receipt、明示承認を指定して`--phase execute --confirmation-relayer-identity <name>`を実行する。

Gate Bにはcleanなmanifest sourceからprofile非依存で生成したUI code/assetsの全file digestとaggregate digestを持つ`ui-assets.json`を必須登録する。activation driverは同じsourceから再buildしてreceipt一致を確認する。production UI deployはこのartifact集合だけを再生成し、検証済みGate Bからrenderした`ui-runtime-profile.json`を`deployment-profile.js`へ直前合成して公開する。dirty checkout、asset追加・欠落・hash drift、bundle外profileはすべて拒否する。

BaseScanのsource verification、contract-created BSNSのownership確認、Token Update申請は[`token-publication.md`](token-publication.md)に従う。この外部申請と審査はGate A、Gate B、activationの認可条件ではない。

deploy、controller handover、activation schedule/executeの固定driverは、不可逆操作の直前にclean sourceから`scripts/ci-local.sh proofs`を再実行する。
proof失敗、実行前後のsource/tree/submodule drift、またはobsoleteな`proof-attestation.json`を含むbundleはfail closedとする。

`execute`提出前はproofと再build後のattestation更新・`verify-live`に続けて`verify-schedule-receipt-live`を実行し、schedule receipt内部のdigest、認証済みSNS proposal/function registry、Canisterのpending operationを再照合する。その後、Base両flowのunpause確定後にCanisterがICをresumeする。ProductionのBase状態は公式EVM RPC Canisterの`BaseMainnet`観測を保存したactivation attestationと認証済みCanister queryで確認し、直接Custom RPC URLは使用しない。3-provider直接照合はstaging monitor drillだけに限定する。
- Holdの強制解除、nonce操作、任意transaction送信は行わない。
## Mint証拠不一致

ownerのRefund請求で`isDepositProcessed(depositId) == true`なのに、`DepositMinted` eventがない、複数ある、Authorization digest・recipient・amount・feeが異なる、またはcanonical成功receiptへ束縛できない場合、Canisterは資金を動かさずfail closedにする。返金や別Authorization発行へfallbackしない。

監査ではDeposit ID、Authorization digest、作成元block、観測Finalized head、runtime hash、signer、epoch、RPC providerの不一致内容を保存する。独立RPCでcontract storage、logs、receipt、canonical block hashを確認し、原因解消後に任意の非anonymous Principalから`request_deposit_refund`を再実行する。record、counter、証拠を手作業で変更しない。

## Stable Settlement executorと手動復旧

Mint Authorizationは作成元Finalized timestampから固定2時間（7,200秒）の期限を持つ。新規Depositなどが取得したFinalized snapshotでdeadline順indexを上限付きに走査し、`Finalized timestamp > deadline`の予約だけを個別RPCなしで解放する。Depositごとのtimer、自動Base照合、自動Ledger返金はない。任意の非anonymous Principalが`request_deposit_refund`を実行すると、認可発行済みDepositの`isDepositProcessed == false`をcanonical blockで確認して固定宛先へ返金する。期限前、等値、RPC不一致では資金を動かさない。

`settlement_scheduler.health = Degraded`の場合はstopped、5分以上overdueのschedule、expired leaseを特定する。active leaseがある間の次回起床はlease期限であり、別のoverdue jobへ即時timerを再armしない。`Faulted`の場合は`last_internal_error`と`last_dispatcher_run_at_ns`を記録し、新規DepositをpauseしてSQLiteを手作業で変更せず、同じWasmをupgradeしてstable job tableからtimerを再armする。改善しなければ障害Wasmとして調査する。
一時障害の基準retry間隔は公開設定`settlement_retry_interval_seconds`であり、Governance transactionの監視設定とは独立している。

RPC障害では複数providerの応答一致とcanonical Finalized headを回復させてから停止recordを再実行する。expired leaseでは保存済みAuthorization digestまたはLedger transfer identityが変わっていないことを確認する。Mint用raw transactionやnonceは存在しない。

Base burnが未通知なら、Historyの`Check and notify`でCanisterのFinalized receipt検証と通知を一回だけ実行する。
productionではUI操作に依存せず、運用者とfailure domainが異なる2系統のkeeperが非終端Withdrawalを監視し、permissionlessな`continue_withdrawal`を1 external stepずつ進める。Gate Bは実burnから`Paid`までのdrill、最大未処理時間、片系停止時の他系継続、manual fallback実施を`keeper-drill.json`で要求する。`monitoring-receipt.json`は同じWithdrawalのFinalized Base receipt／`WithdrawalCommitted` eventと、署名検証付き`get_withdrawal` queryの`Paid`応答を束縛し、そのartifact digestを`keeper-drill.json`から参照する。両系停止または最大未処理時間超過時は新規受付をpauseし、既存債務用cycle reserveを維持してmanual fallbackを開始する。
Withdrawal transaction hashはactive deploymentに束縛したpending confirmationとしてbrowser localStorageへ保存する。recovery cursorは保存しない。回復はWithdrawal Historyの明示的な`Refresh`と必要な回数の`Scan older`でFinalized Base eventを取得し、event行の`Check and notify`から同じhashを通知する。
Deposit mint transaction hashはactive deploymentに束縛して保存する。HistoryはFinalized `DepositMinted` logとCanister DepositをIDで統合し、exact Authorization fieldが一致する成功を復元する。成功後のIC wallet署名やCanister通知はない。
Depositの不明応答はbrowser storageへ保存しない。`Refresh`でowner sequenceとHistoryを読み、受付済みrecordがあれば状態を表示し、未受付なら同じ次sequenceで再度明示送信する。

Deposit refundは任意の非anonymous Principalが請求できる。refund先、金額、Ledger transfer identity、service fee、固定Ledger Feeは保存済みrecordだけから取得し、callerは変更できない。EVM RPCが必要なrefundは外部検証の開始前に手動Retry quotaを消費し、RPC失敗、`NotClaimable`、競合でも返還しない。Withdrawalのstoppedまたは非終端recordには、停止原因の解消後に任意の非anonymous identityから`continue_withdrawal`を一度実行する。UIでは各操作に対応する接続済みIC identityを使用する。別recordのactive lease中は`Busy`、quota超過は`RateLimited`、外部呼出し用cyclesがfloorを割る場合は`InsufficientCycles`を返す。いずれもtimerへ再予約しない。
fee payoutは既存のpayout権限で`continue_fee_payout(payout_id)`を実行する。

- Governance nonceを確保したoperation: `governance-relayer status`で署名成果物を取得し、同じrawを送信・確定する。必要時だけ明示的replacementを要求する。nonceやstable counterを手作業で変更しない。
- Mint Authorization: signatureが未完成なら同一digestを再署名する。`AuthorizationAvailable`ならBase walletで期限内に送信し、成功receipt/eventはUIが追跡する。deadlineを変えた再発行は行わない。
- Withdrawal Ledger hold: `continue_withdrawal`で同一Withdrawal ID・IC Account・固定amountOutを維持する。1呼出しにつきLedger送金またはreconciliationを最大1 external stepだけ進める。dedup期間内は同一transfer identityを一度だけ再送し、完全な不在証拠後は新identityを保存して終了する。新identityの送金は次の明示呼出しで行う。送金先変更、任意送金、Base refundは行わない。
- Deposit funding hold: pullの成功証拠または完全な不存在証明まで補償を行わない。成功時は`EscrowedUnquoted`、不存在時は`Cancelled`へ進める。
- Deposit refund hold: 元account、attemptに保存した金額と固定Ledger feeを照合する。認可発行前は`gross - ledger_fee`、発行後は`gross - charged_service_fee - ledger_fee`で、初回pull fee・確定service fee・refund feeは返さない。成功証拠で`Refunded`へ進め、曖昧結果は任意の非anonymous callerの再請求で照合する。完全な不存在証明後だけattempt番号、created-at time、memoを更新する。確定的な`BadFee`は固定fee設定の不一致として停止し、返金payloadを変更しない。
- 停止理由: Historyまたは`get_deposit`/`get_withdrawal`の`last_settlement_stop_reason`を記録し、外部障害を解消してからContinueする。

手動Retryの既定quotaは10分windowあたりglobal 60、caller 6、record 3である。profile値を変更する場合は`1 <= per_record <= per_principal <= global`とwindow 60〜3600秒を維持する。
