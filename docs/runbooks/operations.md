# Bridge資源補充・緊急停止

## 日常確認

- Bridgeのcycles、signer ETH、reserve surplus、Finalized観測時刻、pending nonce、停止理由を確認する。さらに未決済Withdrawal件数・`amountOut`合計・最古観測時刻・Ledger停止理由を確認する。
- Fee Recipient、RPC credential、raw transaction、秘密情報を監視ログへ出さない。
- `get_bridge_status.counts`の`active_evm_payloads`、`retained_audit_events`、`pruned_audit_events`、`retained_deposit_index_entries`を確認する。audit詳細は直近10,000件、Deposit一覧はownerごとに直近100件が上限である。

Status画面とruntime事前検証は、ブラウザからreview済みprofileのBase RPCへ直接問い合わせる表示用観測であり、CanisterのHTTP outcallを発生させない。表示用availabilityはBaseのFinalized/Safe signer ETH残高、Base pause、IC pause、Canisterのcycles floorを組み合わせ、いずれかが60秒を超えた場合はlast-known値を残したままfail closedにする。Deposit、Withdrawal、Governanceなど資産状態を変える最終判断では、このブラウザ観測を信用せず、Canisterが2-of-3 quorumでBaseを再検証する。

CanisterがFinalized headを取得する際のblock response上限は固定16 KiBである。上限超過時は応答上限の自動拡大や自動再試行をせず、RPC unavailableとしてLedger処理前にfail closedにする。receipt blockは取得せず、2-of-3で一致したreceipt hashへ4 KiB上限の`bridgeSnapshot()` EIP-1898 probeを実行し、`requireCanonical=true`とsnapshotのblock numberでcanonical receiptを確認する。
本番preflightも、receipt、deployment、保存snapshot、Timelock role eventの既知block hashをBridgeの`bridgeSnapshot()`またはTimelockの`getMinDelay()`へEIP-1898で固定する。番号指定block取得は行わず、full block応答はFinalized headのhash発見だけに使う。
2026-07-23のBase Sepolia検証では、直近256 Finalized blockの`eth_getBlockByNumber`応答は最大5,542 bytesであり、16 KiB上限内に収まった。

Canisterが使用するLedger feeの単一の定義元は`canister/bridge-canister/src/ledger.rs`の`KINIC_LEDGER_FEE`である。
Canisterの全Ledger処理がこの値を使い、UIは`get_public_config().ledger_fee`をqueryして同じ値を表示し、事前検証へ使う。

Plan 007のSepolia stagingはKINICではなくTICRC1を使用するため、現在のtest-only値は`10000` rawである。
KINIC mainnet Ledgerのfeeは`100000` rawであり、stagingとの差は意図した環境差である。
詳しい検証条件は`sepolia-staging-e2e.md`の「Test Ledgerのfee」に記載する。

production artifactへstaging Wasmを流用しない。
production buildでは定数をKINIC mainnet Ledgerのlive feeと承認済みprofileへ同期し、Candid binding、Rust/UI/integration test、production preflightを同じ変更で更新する。

stable schemaはv24、record wireはv20を唯一の現行形式とする。未本番期間は現行schema定義を直接置換し、migrationや旧wire fallbackを持たない。旧形式が残るdevelopment/staging Canisterはupgradeせずreinstallする。

## 保持制限と監査

`get_audit_events`で指定したsequenceがpruning済みの場合、応答は`oldest_available_sequence`から始まる。削除済みeventの完全な内容はcanister内に残らず、`pruned_count`、`pruned_through_sequence`、`pruned_digest`だけがcommitmentとして残る。今回は外部archiveや第三者timestampingを行わないため、詳細の長期保管が必要な運用では上限到達前に別系統へ取得する。

`list_deposit_ids.history_truncated = true`はownerの古い一覧索引が削除済みであることを示す。`oldest_available_cursor`より古いDepositでも既知IDによる`get_deposit`と同一requestの冪等retryは利用できる。

schema v24またはwire v20以外のstable state、未知schema、decode不能なDBは、空であってもfail closedで起動を拒否する。

`get_bridge_status.withdrawal_fee_guard_active`がtrueになった場合は、Base Bridgeのwithdrawalを直ちにpauseする。該当recordの`last_settlement_stop_reason`と監査eventに`LedgerFeeExceedsServiceFee`が残り、IC releaseやreserve変更は行われない。Ledger feeとService Feeをreview済みprofileへ同期した後、対象ownerまたは運用principalがHistoryから`continue_withdrawal`を実行する。Canisterが最新Ledger feeを再取得し、charged Service Fee以下であることを確認した場合だけ、同じrecordからreleaseを開始してguardを解除する。
本番未デプロイ期間の開発・テストcanisterで旧schemaが残っている場合はupgradeせずreinstallする。
SQLite DBやcounterを手作業で変更しない。

schema versionの正本は`bridge_metadata.application_schema_version`だけである。Depositはrecord、owner sequence、Base recipientを一つのstable envelopeへ保存し、別intent tableを持たない。pending EVM、open reconciliation hold、nonterminal Withdrawalの件数は各indexの`table_counts`を正本とし、Withdrawal primary rowとliability index・合計額・stop reason集計は一つのSQLite transactionで更新する。

## Controller限定のstorage保守

保守APIはCanister controllerだけが呼び出せる。Governance principal、pause principal、Runtime Administratorには許可しない。実行前に対象Wasmとstable imageを保存し、処理失敗時にmetadataやDBを手作業で変更しない。

新規install後、controller handover前にcontroller identityから`initialize_public_config()`を一度実行する。これはMint SignerとGovernance Operatorをchain-keyから導出し、両方が成功した場合だけstable stateへ一括保存する。続けて`get_public_config()` queryを実行し、release profileの両アドレスと一致することを確認する。初期化失敗、未初期化query、保存済みアドレスとの不一致はいずれもdeployment blockerであり、空値や手入力値で代替しない。upgradeでは保存値を維持し、再初期化を必須としない。

1. `start_storage_validation()`を一度呼び、`continue_storage_validation(100)`を`complete = true`まで反復する。途中で通常更新が入って`StateChanged`になった場合、古いprogressは破棄されるため、静穏時間帯に明示的に再開始する。
2. `storage_integrity_check()` queryが`ok`を返すことを確認する。upgrade処理からこの検査は自動実行されない。
3. `refresh_storage_checksum(4194304)`を`complete = true`まで反復する。一回の呼出しは最大4 MiBであり、raw stable-memoryコピーやfilesystem backupとして扱わない。

旧schemaのlocal/staging Canisterはupgradeせず再作成する。WAL、mmap、compaction、旧`ic-sqlite-vfs`からの直接移行は行わない。

## ETH・cycles補充

- ETHはprofileのthreshold signer addressへ、Settlement Reserveを上回るまで運用者が送る。SNS-token feeの自動交換は行わない。
- cyclesはBridgeの30日floorとfreezing thresholdの両方を満たすことを確認する。
- 補充後も自動resumeしない。Governanceが観測回復と資産状態を確認してからBridgeをresumeする。

## EVM RPC provider

本番Base MainnetのCanister outcallは公式EVM RPC Canisterの組み込み`BaseMainnet` provider群を使い、初期化値`custom_evm_rpc_urls`は空配列とする。公開設定の`rpc_provider_urls_sha256`は実際のcustom URL配列を表すため、本番の正規値は`SHA-256("[]")`である。production profileの3件のcredential-free RPCはCanisterへ注入せず、Gate A、live preflight、UI監視で公式経路から独立して状態を検証するためだけに使う。Base Sepolia stagingと故障演習は従来どおりcustom provider 3件を使う。

## 緊急pause

監視SLOは同一の障害起点から5分以内の検知、15分以内の担当者確認、60分以内のBaseとIC双方のpauseである。片側だけのpauseで完了扱いにしない。単一emergency pause principalの実request IDとaudit event、両transaction/callの確定時刻をevidenceへ入れる。SLO未達、証跡欠落、公式EVM RPC Canister IDまたはchainの不一致、Canisterによるpending Timelock cancel不能はいずれもproduction承認blockerである。EVM RPC Canister配下providerの運営主体・基盤・可用性は外部仮定として扱う。

- 承認済みpause principal identityから`pause_new_deposits`を実行する。既にSubmittedのEVM operationはフロントからのconfirmationを待ち続けるため、各確認待ちと停止理由も監視する。
- Base側の異常では単一emergency pause principalがCanisterの`emergency_pause`を呼び、Canister由来Governance OperatorがDeposit/Withdrawal pauseと記録済みTimelock cancelを送信する。unpauseはSNS proposalからCanisterを経由してTimelockで実行し、limitは変更しない。
- 緊急pauseと競合した未送信のfee・schedule・execute操作は破棄され、未使用nonceだけが再利用される。RPC送信開始済みの操作は`Broadcasting`として保持し、同じhashの存在またはpending nonce前進をquorumで確認できるまで再送・破棄しない。既送信executeが成功しても緊急actionが残る間はICをresumeせず、Base pauseを優先する。
- 再開ごとの`schedule_activation`はCanisterの単調増加operation IDから新しいTimelock saltを導出する。過去に完了したactivationを再利用せず、保存されたoperation IDとsaltの組だけを`execute_activation`またはcancelへ渡す。
- Governance Operatorのnonce競合時は、Canisterが自分のtransaction hashをquorum RPCで再確認する。hashが存在すればconfirmationへ戻り、存在せずpending nonceだけが前進していれば競合操作を失敗終了してnonceを前進させる。hashまたはnonceの証拠が一致しない間は新しい署名を作らず停止する。

本番資産受付は、Gate Aで両Bridgeをpause配置し、Canister controllerを承認済みSNS Rootへhandoverした後に進める。handover後のfresh snapshotでprofile、Canister公開設定、Finalized Base stateのMint Signer一致を確認してGate Bを作り、`production-release.sh activate --phase schedule`で固定SNS proposalを提出する。提出応答だけでは完了扱いにせず、`bridge-profile verify-activation schedule`がSNS実行状態、Canisterのpending operation、Base TimelockのFinalized pending状態を束縛したschedule receiptを発行するまでpauseを維持する。72時間後は古いGate Bを再利用せず、最新Finalized stateからsnapshotを再取得して新しいGate Bを作り、schedule receiptと明示承認を指定して`--phase execute`を実行する。

deploy、controller handover、activation schedule/executeの固定driverは、不可逆操作の直前にclean sourceから`scripts/ci-local.sh proofs`を再実行する。
proof失敗、実行前後のsource/tree/submodule drift、またはobsoleteな`proof-attestation.json`を含むbundleはfail closedとする。

`execute`提出前に`verify-schedule-receipt-live`がschedule receipt内部のdigest、認証済みSNS proposal/function registry、Canisterのpending operation、Base TimelockのFinalized pending状態を再照合する。その後、Base両flowのunpause確定後にCanisterがICをresumeする。proposalの`Executed`表示だけではCandidのdomain errorや後続EVM失敗を除外できないため、`bridge-profile verify-activation execute`がprior schedule receipt、認証済みCanister状態、Base Timelock done、Base/IC双方のunpauseを照合してexecute receiptを発行するまで受付開始を完了扱いにしない。独立した人間EVM管理walletやPause Guardianは存在せず、release driverも失敗時のBase再pause成功を保証しない。Canister、threshold signing、cycles、EVM RPCの相関障害ではBase再pauseが不能になりうるため、検証失敗を成功扱いにせず直ちにincident対応し、BaseとICのlive状態を3-provider quorumと認証済みCanister queryで確認する。
- Holdの強制解除、nonce操作、任意transaction送信は行わない。
## Confirmed EVM revert

Finalized receiptがrevertを示した場合、Bridgeは対象のDeposit EVM operationを`Reverted`へ終端化し、新規Depositを自動pauseする。WithdrawalはEVM operationを生成しない。監査ログのoperation ID、kind、transaction hash、Finalized確認headを保存する。未解決revertが1件でもある間は`resume_new_deposits`は`UnresolvedEvmRevert`を返す。

reverted transactionは自動再送しない。原因とBase状態を独立RPCでも確認した後、Governance principalがDeposit IDと監査ログ上の最新reverted operation IDを指定して`recover_mint_revert`を一度実行する。非Governance caller、operation ID不一致、Bridge signer・runtime不一致、Finalized provider不一致、deposit処理済み、pause、Mint Window、reserve違反では外部署名・broadcast・stable writeを行わない。

`Enqueued`では返されたreplacement operation IDとFinalized block/hashを記録し、フロントの確認待ち一覧へreplacement transactionが復元されることを確認する。
同じ旧operationへの再呼出しが`AlreadyStarted`を返した場合は新しいoperationを作らない。
replacementもrevertした場合、次回は監査ログに記録されたreplacement operation IDを指定する。
`unresolved_evm_reverts`が0になったことを確認するまで`resume_new_deposits`を実行しない。
schema、counter、recordを手作業で書き換えず、旧schemaの開発canisterはupgradeではなくreinstallする。

## Stable Settlement executorと手動復旧

Submitted EVM transactionはCanister内で起床時刻を持たず、フロントからのconfirmationまで待機する。
フロントはpublic Base RPCでtransaction receiptとFinalized headを定期観測する。
`Finalized block >= receipt block`になった場合、認証済みIC walletから専用confirmation APIを呼び、walletのconsent画面でtransactionとblock番号を確認する。
Canisterは受け取った証拠を保存済みtransactionと照合した後、EVM RPC quorumへoutcallしてcanonical receiptとFinalized到達を再検証する。
ブラウザが閉じている間のfallbackはなく、次回起動とIC wallet接続後にlocal pending一覧、またはHistoryのSubmitted状態から監視を再開する。

`settlement_scheduler.health = Degraded`の場合はstopped、5分以上overdueのschedule、expired leaseを特定する。active leaseがある間の次回起床はlease期限であり、別のoverdue jobへ即時timerを再armしない。`Faulted`の場合は`last_internal_error`と`last_dispatcher_run_at_ns`を記録し、新規DepositをpauseしてSQLiteを手作業で変更せず、同じWasmをupgradeしてstable job tableからtimerを再armする。改善しなければ障害Wasmとして調査する。

RPC障害では複数providerの応答一致とcanonical Finalized headを回復させてから、停止したrecordのconfirmationをフロントまたはHistoryから再実行する。
expired leaseは外部callの結果が不明な可能性があるため、保存済みraw transactionまたはLedger transfer identityが変わっていないことを確認する。
Finalized revertは前節の手順に従い、手動Retryしない。

Base burnが未通知なら、Historyの`Check and notify`でCanisterのFinalized receipt検証と通知を一回だけ実行する。
Withdrawal transaction hashはactive deploymentに束縛したpending confirmationとしてbrowser localStorageへ保存する。recovery cursorは保存しない。回復はWithdrawal Historyの明示的な`Refresh`と必要な回数の`Scan older`でFinalized Base eventを取得し、event行の`Check and notify`から同じhashを通知する。
Depositの不明応答はbrowser storageへ保存しない。`Refresh`でowner sequenceとHistoryを読み、受付済みrecordがあればそれをContinueし、未受付なら同じ次sequenceで再度明示送信する。

所有者が操作できない場合、Governanceまたはpause administratorは停止原因の解消を確認する。
Submitted EVM operationにはtransaction hash、receipt block、観測Finalized blockを指定して`confirm_deposit`を一度実行する。
その他のstopped、expired、または非終端なのにjobがないrecordには`continue_deposit`または`continue_withdrawal`を一度実行する。
Submitted状態へのContinueは`ConfirmationRequired`、別recordのactive lease中は`Busy`、10分windowの上限超過は`RateLimited`を返す。
fee payoutは既存のpayout権限で`continue_fee_payout(payout_id)`を実行する。

- nonceを確保したoperation: 対象recordのContinueを実行する。別recordが`NonceBlocked`なら、先にnonceを保持するSubmitted/Prepared operationを特定してContinueする。nonceやstable counterを手作業で変更しない。
- Withdrawal Ledger hold: `continue_withdrawal`で同一Withdrawal ID・IC Account・固定amountOutを維持する。dedup期間内は同一transfer identityを一度だけ再送し、期間後は一回につきreconciliationを1 stepだけ進める。完全な不在証拠なしに別identityを作らない。送金先変更、任意送金、Base refundは行わない。
- Deposit funding hold: pullの成功証拠または完全な不存在証明まで補償を行わない。成功時は`EscrowedUnquoted`、不存在時は`Cancelled`へ進める。
- Deposit refund hold: 元account、attemptに保存した`gross - ledger_fee`、feeを照合する。成功証拠で`Refunded`へ進め、曖昧結果は完全な不存在証明後だけattempt番号、created-at time、memoを更新する。確定的な`BadFee`は固定fee設定の不一致として停止し、返金payloadを変更しない。Ledger feeを変更せず、Canister設定とLedger設定の不一致を解消してから同じrecordを再実行する。
- Submitted EVM transaction: Continueを使わない。Finalized到達を観測した後、保存済みtransaction hashと一致する証拠で専用confirmation APIを呼ぶ。
- 停止理由: Historyまたは`get_deposit`/`get_withdrawal`の`last_settlement_stop_reason`を記録し、外部障害を解消してからContinueする。

手動Retryの既定quotaは10分windowあたりglobal 60、caller 6、record 3である。profile値を変更する場合は`1 <= per_record <= per_principal <= global`とwindow 60〜3600秒を維持する。
