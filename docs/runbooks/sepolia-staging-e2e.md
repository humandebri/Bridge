# IC mainnet × Base Sepolia staging E2E

このrunbookの対象は、現在のstaging Canister、deployment instance、Timelock、Bridge、bSNS、signerで構成される同一のlive stackである。2026-08-27/28に完了したdestructive reinstallとfresh-stack構築は一度限りの履歴としてhash固定し、再実行、resume、別stack構築の入力には使わない。今後のCanister更新はstable schema v35／record wire v30を保つsame-instance `upgrade`だけを許可する。

Base Sepolia stagingは`short-delay-test-only` policyによりTimelock delayを300秒とする。productionの259200秒制約は変更せず、短縮版artifactや証跡をproduction rehearsalへ使用しない。production Canister、KINIC Ledger、Base Mainnet、SNSを対象にしてはならない。

## Test Ledgerのfee

stagingは現在binding済みの共有TICRC1 Ledger/Indexを維持する。`test-deployment` WasmはTICRC1の`icrc1_fee()`と一致する`KINIC_LEDGER_FEE = 10000`を使い、production Wasmの`100000`とは分離する。staging artifactをproductionへ流用しない。

## 証跡世代と初期化

- 現行state machineはevidence schema v8だけを受理する。v7 manifest、local evidence、artifactは監査用の読取専用履歴であり、resume、migration、dual-read、合格判定を行わない。
- `scripts/plan007-local-gate.sh /secure/work/local-e2e.json`をclean commitで実行し、schema v8 local evidenceをrepository外へ発行する。既存のchecked-in v7 `local-e2e.json`を更新または再利用しない。
- `BRIDGE_STAGING_LOCAL_EVIDENCE=/secure/work/local-e2e.json scripts/plan007/staging-e2e-driver.sh init`でv8 manifestを新規作成する。過去の証跡を遡及的に`SHORT_DELAY_LIVE`へ変更しない。
- `bootstrap_attestation`は`evidence/reinstall-decision-2026-08-27.json`と`evidence/fresh-stack-2026-08-28.json`をhash固定し、現行bindingとの一致と履歴が再開不能であることだけを確認する。reinstall、contract deploy、activationは実行しない。

`staging-e2e-driver.sh`はartifactを検証してmanifestへ記録するread-only recorderであり、Canister、contract、frontendを変更しない。外部upgrade、cycles投入、Base Sepolia transaction、frontend公開は、それぞれ別の明示承認とreview済み専用toolingの後にだけ実行する。生の`icp canister install`をrunbook手順として実行せず、`install`、`reinstall`、`auto`を受け付ける汎用経路を追加しない。秘密鍵、identity名、RPC credentialはrepositoryへ保存しない。

## 証跡state machine

```text
bootstrap_attestation
  -> preflight
  -> current_schema_upgrade
  -> post_upgrade_binding
  -> frontend_publish
  -> smoke_e2e
  -> wallet_e2e
  -> refund_rehearsal
  -> rpc_rehearsal
  -> live_acceptance
  -> SHORT_DELAY_LIVE
```

各stageはsource commit、artifact SHA-256、実行対象、raw receipt、観測postconditionを保存する。upgrade、binding、frontend、smoke、wallet、refundはstage receiptだけでは受理せず、tool/argv/exit code/raw JSON stdoutとそのdigestを持つstage固有`*-raw-capture`を必須とする。validatorはstdoutを再parseし、stage details、`details_sha256`、`capture_sha256`が一致することを確認する。RPC summaryは`rpc-rehearsal-manifest`のdigestと専用verifier結果へ結合し、`live_acceptance`は`reactivation-schedule-receipt`、`reactivation-execute-receipt`、`staging-monitoring-receipt`の3 artifactへ結合する。失敗commandの出力をPASS証跡にしない。

`preflight`はlive queryからCanister ID、deployment instance、module、controller、cycles、schema/wire、minimum Withdrawal ID、storage integrity、state count、固定RPC順序とprovider chain bindingを検査する。profileの`rpcProviderUrlsSha256`をlive RuntimeBindingの集約digestへ一致させ、3 providerの個別URL digest順序を後続RPC rehearsalの`rpc_endpoints`へ結合する。Base Deposit/WithdrawalとCanister Depositはlive状態でunpausedでなければならない。

`current_schema_upgrade`はcanonical test-deployment Wasm `target/test-deployment/staging/bridge_canister.wasm`を`upgrade` modeで適用した証跡だけを受理する。次をすべて満たさなければならない。

- Canister ID、deployment instance、stable schema v35、record wire v30、minimum Withdrawal IDが不変。
- module/Candid hashがreview済みbindingと一致。
- controller集合と全state countが前後で一致し、storage integrityが`ok`。
- `reinstall`、`auto`、instance drift、旧・未知schema、未登録wireを拒否。

`post_upgrade_binding`は同じidentity、固定RPC provider集約digest、runtime/contract bindingをlive再取得して確認する。`rpc_rehearsal`は独立したRPC rehearsal schemaに従い、最後の`final_pause`まで含める。これはouter staging stateの終端ではない。`live_acceptance`は別operation IDのreactivation schedule/execute Finalized receiptを検証し、IC/Baseの資産フローが再びunpausedであることを確認する。

## Finality遅延、Wallet、Refund E2E

- Finalized headを意図的に約20分遅らせた状態でDeposit reviewが成功し、authorizationの`issued_at_timestamp`と`deadline = issued_at + 600`を記録する。
- quote snapshotの`blockTimestamp`を正本として、残り300秒では送信可能、299秒以下とwindow終端ちょうどではwallet呼出し、Ledger pull、intent保存、Base transaction送信を開始しないことを記録する。
- 実walletでTICRC1 DepositからBase mintまで完了し、Base ETH支払receiptとexact processed Depositを記録する。
- `AuthorizationExpired`と`AuthorizationWindowTooShort`はいずれも停止理由としてHistoryへ伝播する。Finalized Base timestampがdeadlineを超えるまではrefund不可で、超過後のexact未処理証拠でだけrefundできることを記録する。
- Solidityは`block.timestamp + 900`を受理し、`+901`を拒否することを対象artifactとtransaction testで固定する。

## RPC故障演習

RPC順序はPublicNode、`sepolia.base.org`、dRPCに固定し、事前chain bindingとruntime 2-of-3 quorumを確認する。単一provider failureは継続し、quorum loss、chain mismatch、canonical receipt不一致はfail closedにする。runtimeでRPC URLまたはchain IDを可変にせず、runtime `eth_chainId` callは追加しない。

## `SHORT_DELAY_LIVE`受入条件

新しいv8 manifestだけが、次をすべて満たした場合に`SHORT_DELAY_LIVE`へ遷移する。

- Base Deposit/WithdrawalとCanister Depositがunpaused。
- Canister ID、module、schema v35、wire v30、deployment instance、minimum Withdrawal ID、contract/runtime/profile hashが一致し、storage integrityが`ok`。
- historical/retired stack identityがactive profile、signer、automationから排除されている。
- pending governance、Timelock、Deposit、Withdrawal、Ledger operation、reconciliation、mint reservation、unpaid liabilityがすべて0。
- 固定RPC providerの事前chain bindingとhealthが正常。
- wallet E2E、refund rehearsal、RPC rehearsalが成功。
- reactivation schedule/execute receipt、監視receipt、Finalized block/hashが保存されている。

受入後もstagingをpauseせず、資産フローを有効に保つ。実際のCanister upgrade、frontend publish、Base transaction、受入証跡の確定はこのコード変更には含めず、別途明示承認後に実行する。Production配置もこのrunbookに含めない。
