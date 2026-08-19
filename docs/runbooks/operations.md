# Bridge資源補充・緊急停止

## Schema v33 baseline

初回mainnet deployで導入するv33は、capacity reservationと`nonterminal_deposit_owner_index`を含む現在のSQLite形状だけを正本とする。同じversion番号を持つそれ以前の開発DBとは互換性を持たず、tableとcounterの欠落はreopen時にfail closedとなる。初回mainnet deploy完了後はこの形状をproduction baselineとして固定し、以後の形状変更はschema番号を上げた明示migrationとして扱う。

## 日常確認

- Bridgeのcycles、Governance Operator ETH、governance reserve surplus、Finalized観測時刻、Governance pending nonce、停止理由を確認する。さらに未決済Authorization、未決済Withdrawal件数・`amountOut`合計・最古観測時刻・Ledger停止理由を確認する。Mint Signer ETHは要件ではない。
- Fee Recipient、RPC credential、raw transaction、秘密情報を監視ログへ出さない。
- `get_bridge_status.counts`の`reserved_deposit_mint_operations`、`reserved_deposit_mint_amount`、`pending_ledger_operations`、`retained_audit_events`、`pruned_audit_events`、`retained_deposit_index_entries`と`audit_retention_warning`を確認する。audit詳細は直近100,000件で、80,000件以上は警告する。通常Deposit一覧はownerごとに直近100件だが、非終端Depositは独立indexから全件をpagination取得できる。

Status画面とruntime事前検証は、ブラウザからreview済みprofileのBase RPCへ直接問い合わせる表示用観測であり、CanisterのHTTP outcallを発生させない。Deposit表示用availabilityはBase Finalized/Safe、runtime、pause、epoch、IC pause、Canister cycles floorを組み合わせ、Mint Signer ETH残高を条件にしない。Governance availabilityだけはGovernance Operator ETH floorを確認する。いずれかが60秒を超えた場合はlast-known値を残したままfail closedにする。資産状態を変える最終判断ではブラウザ観測を信用せず、Canisterがprovider quorumでBaseを再検証する。

CanisterがFinalized headを取得する際のblock response上限は固定16 KiBである。上限超過時は応答上限の自動拡大や自動再試行をせず、RPC unavailableとしてLedger処理前にfail closedにする。receipt blockは取得せず、2-of-3で一致したreceipt hashへ4 KiB上限の`bridgeSnapshot()` EIP-1898 probeを実行し、`requireCanonical=true`とsnapshotのblock numberでcanonical receiptを確認する。
本番preflightも、receipt、deployment、保存snapshot、Timelock role eventの既知block hashをBridgeの`bridgeSnapshot()`またはTimelockの`getMinDelay()`へEIP-1898で固定する。番号指定block取得は行わず、full block応答はFinalized headのhash発見だけに使う。
2026-07-23のBase Sepolia検証では、直近256 Finalized blockの`eth_getBlockByNumber`応答は最大5,542 bytesであり、16 KiB上限内に収まった。

Canisterが使用するLedger feeの単一の定義元は`canister/bridge-canister/src/ledger.rs`の`KINIC_LEDGER_FEE`である。
Canisterの全Ledger処理がこの値を使い、UIは`get_public_config().ledger_fee`をqueryして同じ値を表示し、事前検証へ使う。

production Canisterが受け入れるLedger feeは`100000` raw、`test-deployment` featureで作るstaging Canisterは`10000` rawに固定する。activation preflightとruntimeの`BadFee`処理は、buildが選択した固定値との差異をfail closedにする。
詳しい検証条件は`sepolia-staging-e2e.md`の「Test Ledgerのfee」に記載する。

production artifactへstaging Wasmを流用しない。
production buildでは定数をKINIC mainnet Ledgerのlive feeと承認済みprofileへ同期し、Candid binding、Rust/UI/integration test、production preflightを同じ変更で更新する。

stable schemaはv33、record wireはv28を現行形式とする。`post_upgrade`は現行形式だけを受理し、migration、dual-read、旧wire fallbackは持たない。

## 保持制限と監査

`get_audit_events`で指定したsequenceがpruning済みの場合、応答は`oldest_available_sequence`から始まる。削除済みeventの完全な内容はcanister内に残らず、`pruned_count`、`pruned_through_sequence`、`pruned_digest`だけがcommitmentとして残る。今回は外部archiveや第三者timestampingを行わないため、詳細の長期保管が必要な運用では上限到達前に別系統へ取得する。

`list_deposit_ids.history_truncated = true`はownerの古い一覧索引が削除済みであることを示す。`oldest_available_cursor`より古いDepositでも既知IDによる`get_deposit`と同一requestの冪等retryは利用できる。

schema v33またはwire v28以外のstable state、未知schema、decode不能なDBは、空であってもfail closedで起動を拒否する。

`get_bridge_status.withdrawal_fee_guard_active`がtrueになった場合は、Base Bridgeのwithdrawalを直ちにpauseする。該当recordの`last_settlement_stop_reason`と監査eventに`LedgerFeeExceedsServiceFee`が残り、IC releaseやreserve変更は行われない。buildが選択した固定`KINIC_LEDGER_FEE`（productionは`100000 raw`、stagingは`10000 raw`）とprepared recordのcharged Service Feeをreview済みprofileに照合した後、任意の非anonymous主体がHistoryから`continue_withdrawal`を実行する。Canisterはruntimeで`icrc1_fee()`を照会せず、固定Ledger Feeがcharged Service Fee以下であることを再検証できた場合だけ、同じrecordからreleaseを開始してguardを解除する。
現行の開発・staging・production canisterはstable schema v33／record wire v28だけを受理する。これ以外の形式は空stateであってもfail closedとし、migrationや旧Wasm fixtureを現行release判断へ使用しない。初期化済みの永続Canisterは同一deployment instanceのupgradeだけで更新し、reinstallは禁止する。新しいCanister IDへの初回installはこの制約に含めない。
SQLite DBやcounterを手作業で変更しない。

schema versionの正本は`bridge_metadata.application_schema_version`だけである。Depositはrecord、owner sequence、Base recipient、Authorization、失効またはMint確定証拠を一つのstable envelopeへ保存する。pending Ledger、open reconciliation hold、nonterminal Withdrawalの件数は各indexの`table_counts`を正本とし、primary rowとliability index・集計は一つのSQLite transactionで更新する。

## Controller限定のstorage保守

保守APIはCanister controllerだけが呼び出せる。Governance principal、pause principal、Runtime Administratorには許可しない。実行前に対象Wasmとstable imageを保存し、処理失敗時にmetadataやDBを手作業で変更しない。

新規install後、controller handover前にcontroller identityから`initialize_public_config()`を一度実行する。これはMint SignerとGovernance Operatorをchain-keyから導出し、両方が成功した場合だけstable stateへ一括保存する。続けて`get_public_config()` queryを実行し、release profileの両アドレスと一致することを確認する。初期化失敗、未初期化query、保存済みアドレスとの不一致はいずれもdeployment blockerであり、空値や手入力値で代替しない。upgradeでは保存値を維持し、再初期化を必須としない。

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

本番Base MainnetのCanister outcallは公式EVM RPC Canisterの組み込み`BaseMainnet` provider群を使い、初期化値`custom_evm_rpc_urls`は空配列とする。公開設定の`rpc_provider_urls_sha256`は実際のcustom URL配列を表すため、本番の正規値は`SHA-256("[]")`である。production profileの3件のcredential-free RPCはCanisterへ注入せず、Gate A、live preflight、UI監視で公式経路から独立して状態を検証するためだけに使う。Base Sepolia stagingと故障演習のcustom provider 3件はURL、chain ID、接続先chainを稼働中固定し、deploy・activation前preflightで全3件のchain ID一致を確認する。runtimeの2-of-3 quorumは応答不一致と障害への対策であり、chain切替検知には使用しない。この設計判断と記録の意味論は[ADR 0024](../adr/0024-validate-rpc-chain-binding-before-runtime.md)を正本とする。

## 緊急pause

監視目標は同一の障害起点から5分以内の検知、15分以内の担当者確認、60分以内のBaseとIC双方のpauseである。片側だけのpauseで完了扱いにしない。単一emergency pause principalの実request IDとaudit event、両transaction/callの確定時刻をevidenceへ入れる。本番ゲートはpause/cancel経路の成功、証跡、公式EVM RPC Canister IDとchain、pending Timelock cancel可能性を要求し、5/15/60の実測達成は公開後に評価する。EVM RPC Canister配下providerの運営主体・基盤・可用性は外部仮定として扱う。

- 承認済みpause principal identityから`pause_new_deposits`を実行する。未期限Mint AuthorizationはBase側の`pauseDepositMints`によるepoch増加で失効するが、返金は元deadline後のFinalized未処理証拠まで待つ。各Authorizationのdeadlineと停止理由を監視する。
- Base側の異常では単一emergency pause principalがCanisterの`emergency_pause`を呼ぶ。この呼出しの成功条件はIC側pauseとBase action queueの永続化までであり、Baseへの送信完了ではない。同じpause principal identityを使う外部CLIで`drain-emergency`を実行し、Deposit/Withdrawal pauseと記録済みTimelock cancelを順に署名・送信・確定する。
- 緊急pauseと競合した、まだ署名成果物を発行していないfee・schedule・execute操作だけを破棄して未使用nonceを再利用できる。`SignedAwaitingRelay`は外部送信済みの可能性があるため破棄しない。既送信executeが成功しても緊急actionが残る間はICをresumeせず、Base pauseを優先する。
- 再開ごとの`schedule_activation`はCanisterの単調増加operation IDから新しいTimelock saltを導出する。過去に完了したactivationを再利用せず、保存されたoperation IDとsaltの組だけを`execute_activation`またはcancelへ渡す。
- Governance Operatorのnonce競合時にCanisterは自動回復しない。CLIは発行済みhashを確認し、exact raw transactionの再送または既知transaction応答だけを冪等成功として扱う。別hashによるnonce消費が疑われる場合は停止して監査し、stable nonceを手作業で変更しない。

## Governance relayer

CanisterはBase governance transactionをbroadcastせず、governance timerも持たない。Service Fee、activation schedule/executeにはGovernance principal identityを使う。Base pause、記録済みTimelock cancel、`drain-emergency`にはGovernanceまたはpause principal identityを使える。

```bash
export BRIDGE_CANISTER_ID='...'
export IC_IDENTITY_PEM='/secure/path/governance.pem'
export BASE_RPC_URL='https://...'

npm run governance-relayer -- status
npm run governance-relayer -- prepare --action pause-deposits
npm run governance-relayer -- run
```

`run`はpending署名成果物の取得、raw transactionのhash・chain・sender・nonce・target・calldata・gas・fee検証、broadcast、Finalized待機、Canister確定通知を行う。broadcast直後に停止した場合は`status`で同じoperationを確認して`run --operation-id <id>`を再実行する。同じraw transactionの再送とRPCの`already known`は冪等成功だが、`nonce too low`は成功扱いにしない。

threshold signingの一時障害でoperationが`Prepared`に残ると、`status`は`SigningUnavailable`を返す。通常操作は同じ`prepare --action ...`、activationは同じCanister API、緊急操作は`drain-emergency`を再実行する。再試行は保存済みnonce、target、calldata、feeを変更せず、署名済み成果物がある場合は再署名しない。

詰まり時のreplacementは自動生成しない。現在のhashと新しいfeeを人間が確認し、次を明示実行する。Canisterは同一operation・nonce・target・calldata・gasを維持し、直前generation比12.5%以上、設定ceiling内、最大3回だけ再署名する。CLIは独自署名やfee変更を行わない。

```bash
npm run governance-relayer -- replace \
  --operation-id <id> \
  --max-fee <wei> \
  --priority-fee <wei>
npm run governance-relayer -- run --operation-id <id>
```

`IC_IDENTITY_PEM`はCanisterの認可APIだけに使用し、通常操作ではGovernance identity、緊急pause/cancelではpause identityを指定できる。EVM秘密鍵は用意しない。gasはthreshold Governance Operator EOAが負担する。RPC URL/API keyやPEM内容をログ・shell history・incident evidenceへ記録しない。

stagingを現行schemaへ切り替える前にpending governance transactionとemergency queueが空であることを確認する。schema v33／wire v28以外のcanisterはupgrade対象にせず、現行Wasmを新規installして検証stateを作り直す。rollbackでは最初にrelayerを停止し、同一schemaの対応Wasmとstable snapshotをセットで復元する。

初回production Canister作成は`icp.yaml`へsubnetを設定せず、review済みidentityで`BRIDGE_ICP_IDENTITY=<identity> scripts/production-canister-bootstrap.sh`を実行する。このscriptは`pzp6e-ekpqk-3c5x7-2h6so-njoeq-mt45d-h3h6c-q3mxf-vpeez-fez7a-iae`を`icp canister create --subnet`へ固定し、作成後または既存mapping再利用時にNNS Registryが返す実subnetとの一致を必須にする。`.icp/data/mappings/production.ids.json`に既存IDがある場合は新規作成しない。

本番資産受付は、Gate Aで両Bridgeをpause配置し、Canister controllerを承認済みSNS Rootへhandoverした後に進める。handover後のfresh snapshotでprofile、Canister公開設定、Finalized Base stateのMint Signer一致を確認してGate Bを作り、`production-release.sh activate --phase schedule`で固定SNS proposalを提出する。提出応答だけでは完了扱いにせず、`bridge-profile verify-activation schedule`がSNS実行状態、Canisterのpending operation、Base TimelockのFinalized pending状態を束縛したschedule receiptを発行するまでpauseを維持する。24時間後は古いGate Bを再利用せず、最新Finalized stateからsnapshotを再取得して新しいGate Bを作り、schedule receiptと明示承認を指定して`--phase execute`を実行する。

Gate Bにはcleanなmanifest sourceからprofile非依存で生成したUI code/assetsの全file digestとaggregate digestを持つ`ui-assets.json`を必須登録する。activation driverは同じsourceから再buildしてreceipt一致を確認する。production UI deployはこのartifact集合だけを再生成し、検証済みGate Bからrenderした`ui-runtime-profile.json`を`deployment-profile.js`へ直前合成して公開する。dirty checkout、asset追加・欠落・hash drift、bundle外profileはすべて拒否する。

deploy、controller handover、activation schedule/executeの固定driverは、不可逆操作の直前にclean sourceから`scripts/ci-local.sh proofs`を再実行する。
proof失敗、実行前後のsource/tree/submodule drift、またはobsoleteな`proof-attestation.json`を含むbundleはfail closedとする。

`execute`提出前に`verify-schedule-receipt-live`がschedule receipt内部のdigest、認証済みSNS proposal/function registry、Canisterのpending operation、Base TimelockのFinalized pending状態を再照合する。その後、Base両flowのunpause確定後にCanisterがICをresumeする。proposalの`Executed`表示だけではCandidのdomain errorや後続EVM失敗を除外できないため、`bridge-profile verify-activation execute`がprior schedule receipt、認証済みCanister状態、Base Timelock done、Base/IC双方のunpauseを照合してexecute receiptを発行するまで受付開始を完了扱いにしない。独立した人間EVM管理walletやPause Guardianは存在せず、release driverも失敗時のBase再pause成功を保証しない。Canister、threshold signing、cycles、EVM RPCの相関障害ではBase再pauseが不能になりうるため、検証失敗を成功扱いにせず直ちにincident対応し、BaseとICのlive状態を3-provider quorumと認証済みCanister queryで確認する。
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
