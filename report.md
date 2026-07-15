# KINIC–Base Bridge セキュリティ検証レポート

レビュー日：2026-07-15  
対象：staged、unstaged、untrackedを含む現在の作業ツリー  
前提：本番未デプロイのため、旧ABI、旧Candid、旧stable schemaとの互換処理は持たない

## 結論

対象Withdrawalのterminal liability zeroと複合stable writeは、現行sourceに対するLean・Verus・Solidity SMT、およびRPC監査eventを含む全transaction経路のfailpoint/reopen検査を再実行し、`RETEST PASS`とした。

公式EVM RPC Canisterを経由するBase Sepolia実演習は、実行手順、証跡形式、fail-closed validatorまで実装したが、実ネットワークでは未実施である。
このadvisoryは`PENDING EXTERNAL RETEST`のままとする。

本レポートは「全実装にバグがない」とは主張しない。
機械証明は定義したモデルと`TrustedWorld`として与えた外部条件の範囲に限られ、外部サービス、非同期実行、鍵管理、運用応答の正しさを証明しない。

Safe確認はL1 settlement完了より弱い。ユーザーの`createWithdrawal`をSafe確認した後、finality前reorgによってBase上のburnが消える一方でICP送金が残る可能性は、証明対象外の受容リスクである。

署名済みGate B evidence bundleが完成し、現行sourceで全CIと形式検証が成功するまでproduction deploy blockerを維持する。
controller handoverはGate A後に別の明示承認で実施し、そのlive結果をfresh Gate Bへ含める。
Gate Bが成立してもunpauseと資産受付開始には別の明示承認を要する。

| 対象 | 判定 | 根拠 |
|---|---|---|
| 複合stable write | RETEST PASS | 業務状態とRPC監査eventを単一transaction化し、notify release/cancel、Submitted、NonceConflict、Base snapshot success/loss、terminal各write pointのsnapshot/cache/reopen不変を検査 |
| 対象Withdrawalのterminal liability zero | RETEST PASS | Lean `sorry` 0、Verus 55 verified / 0 errors、Solidity SMT pass/fail fixtureが成功 |
| 公式EVM RPC Canister経由の実E2E | PENDING EXTERNAL RETEST | rehearsalとvalidatorは完成、IC stagingとBase Sepoliaでは未実行 |
| 本番承認 | DEPLOY BLOCKED | 署名済みGate B bundle、ceremony、監視演習が未提示 |

## 複合stable write

### 判定

Deposit mint準備、初期Withdrawal、即時Refund、BadFee cancellation、`ReleaseCancelled`から`RefundPending`への遷移、EVM finalizationとrevert、reconciliation holdからの復帰を単一SQLite transactionへ統合した。

operation IDはread-only candidateとして取得し、transaction内でpersisted counter、旧record状態、payload hash、operation kind、ownerとindexの不在を再検査する。
calldata、CBOR、index key、counterとaccountingの更新値はwrite前に作るため、encode失敗、overflow、stale candidate、conflicting replayでは永続状態を変更しない。

transactionはexecution payload、EVM operation、各index、DepositまたはWithdrawal、counter、accounting、external progress、auditとtable countを一括commitする。
memory cacheはcommit成功後だけ更新し、async `await`を跨いでtransactionや借用状態を保持しない。

### 検証

各複合更新のwrite点へfailpointを設けた。
失敗時はDB snapshot、operation counter、各index、cache、reopen後状態が実行前と一致することをunit testで確認した。

CIはoperation採番後の逐次`put_*`連鎖を静的に拒否する。
stable schemaはv6だけを受理し、旧版と未知版をmigrationなしでfail closedにする。

## Terminal liability zero

### 証明対象

LeanモデルはWithdrawal liabilityを**対象Withdrawal固有liability**と**他Withdrawalのliability**に分け、1:1 invariantでは両者を合算する。
releaseは`amountOut + serviceFee + ledgerFee = 対象liability`、refundは`gross = 対象liability`を前提とし、terminal遷移で対象liabilityを0にする。

`EconomicTerminal := Released ∨ Refunded`として、次の性質を`sorry`なしで証明した。

- 1:1 liability保存
- 非負性
- releaseとrefundの受領履歴
- 同一Withdrawalのreleaseとrefundの排他
- economic terminalに到達した対象Withdrawalのliabilityが0
- 対象Withdrawalの遷移によって他Withdrawalのliabilityが変化しないこと

### 実装との接続

production共有kernelへ厳密なsettlement分割とterminal residual計算を追加した。
このkernelを`Settlement::validate`とWithdrawalのterminal遷移から直接呼び出し、Verusでproduction obligationと対応するnegative fixtureを検査する。

証明は「Bridge全体のliabilityがterminal時に0になる」とは述べない。
他の未完了DepositとWithdrawalはliabilityを保持できるため、0になるのは対象Withdrawal固有liabilityだけである。

活性（すべてのWithdrawalが最終的にterminalへ到達すること）は証明対象外である。
正当な初期状態は`ValidInitial`として定義する。
idle、対象liability 0、未受領履歴、1:1、非負性からterminal safetyを導出するため、terminal liability zeroを初期状態の同一命題として仮定しない。

正直なBridge signer、canonical Safe chain、Ledger結果真正性、IC messageとSQLite transactionの原子性は`WorldAssumptions`と`TrustedWorld`で明示的なrefinement入力にした。
Leanはこれら外部条件そのものを証明せず、条件が成立する`RefinedExecution`についてterminal safetyを証明する。
Safe後からL1 finalityまでのreorg耐性は証明しない。

## EVM RPC Canister実演習

### 実装済みの検査

manual rehearsalはmockを参照せず、IC上のtest Bridge Canister、test ICRC LedgerとIndex、公式EVM RPC Canister、Base Sepolia専用Bridgeを対象とする。
証跡はrequestとresponse、transaction hash、Ledger block、Safe block numberとhash、RPC合意結果を同じrehearsal IDへ結び付ける。

rehearsalは次の10 scenarioが完了しなければ`COMPLETE`にならない。

- Deposit mint
- ユーザーの`createWithdrawal`によるburnと`Releasing`化、ICRC transfer、ack
- BadFee minimum割れ、cancel、refund
- receipt、event、state、Bridge snapshotとcanonical Safe block hashの一致
- 1 provider相当の失敗後もquorumが成立する場合の継続
- quorum不成立時のfail-closed
- `NonceTooLow`でlocal transaction hashが存在する場合と存在しない場合
- 最終的なBaseとICのpause確認

orphan receipt、同じheightでのhash不一致、provider誤応答はPocketIC fixtureで検査する。
公開RPCへの故障注入はproduction承認条件にしない。

### 監査境界

EVM RPC Canister ID、Base chain ID、canonical block numberとhash、quorum結果、Base signerとCanister signerは監査対象に残す。

EVM RPC Canister配下providerの運営法人、upstream、ASN、cloud、region、障害ドメイン、可用性SLOは監査対象外である。
「EVM RPC Canisterと設定されたprovider quorumがcanonical Safe chainを正しく返す」を外部仮定として扱う。

## Evidence bundleと本番Gate

evidence bundle v1は`release-manifest.json`、`profile.json`、`signer-snapshot.json`、`ceremony.json`、`rpc-e2e.json`、`monitor-drill.json`で構成する。
JSONをRFC 8785の対応subsetでcanonicalizeしてSHA-256を計算し、秘密情報は保存しない。

`validate-bundle --offline`はschema、実ファイルhash、source binding、90日の有効期限、監視実測値、EIP-191署名を検査する。
`verify-live`はchain ID、公式EVM RPC Canister ID、canonical結果、profileとBaseとCanisterのsigner、runtime bytecode、Timelock、IC controller、reserve、実rehearsalを検査する。

Gate Aはoffline bundleの検証後にBaseとCanisterを初期pause状態で配置する段階である。
Gate Bはlive状態、Gate A receiptへのbinding、専用hardware walletによる最終manifest署名を検査する段階である。
実行順序はGate A配置、pause維持下のcontroller handover、fresh Gate BによるTimelock schedule、72時間待機、新しいfresh Gate Bによるexecuteである。execute中にIC resumeが失敗した場合はBase両flowを再pauseし、incidentとして終了する。

production scriptはbundle欠落、test bundle、source drift、Gate不足、署名欠落、codeまたはrole driftの場合にbroadcastとunpauseを拒否する。
任意driverや任意preflightへの差し替えも拒否する。

Timelockのrole集合はconstructor完了時に凍結し、自己callを含むgrant、revoke、renounceを拒否する。role変更は、承認済みの新しいrole集合で同一runtimeのTimelockを配置し、既存TimelockからBridge rotationを行う。

Canisterにはrelease IDへ束縛したchain-key challenge署名endpointを追加した。
live preflightは保存済み署名を信用せず、その場で署名を取得し、導出したCanister signerとBase signerを比較する。

監視SLOは同じ障害起点T0から`detect <= 5分`、`human ack <= 15分`、`BaseとIC双方pause <= 60分`を要求する。
片側だけのpauseは成功扱いにしない。

## 保証境界

### 機械証明済みの範囲

- Lean：ユーザー実行型Withdrawal、1:1 liability保存、releaseとrefundの排他、対象Withdrawalのterminal liability zero、他Withdrawal liabilityの不変性。Safe後・finality前reorgは対象外
- Verus：production共有kernelとmodel obligation。現行sourceで55 verified、0 errors
- Solidity SMT：`Releasing`作成、cancel、ack、refund、fee、minimum、Ledger index。pass fixtureのverification conditionと、1 obligation 1 negative fixtureを検査

### テストで検査した範囲

- BridgeとBSNS本体のmapping更新、modifier、mintとburn、EIP-3009、Timelock
- stable transactionのrollback、operation ID競合、index競合、reopen
- PocketIC上の非同期call、timeout、canonical receipt、BadFee、nonce conflict
- UIのwallet状態、profile gate、history pagination、keyboard操作、ARIA
- production wrapper、固定deploy driver、live preflight、source binding

### 外部仮定と未実施事項

- EVM RPC Canisterと設定provider quorumがcanonical Safe chainを正しく返すこと
- Safe確認後からL1 finalityまでreorgしないことは仮定せず、この区間のreorgによる1:1毀損を受容リスクとして残す
- Bridge signerが定めたprotocolだけへ署名すること
- ICRC LedgerとIndexの結果と履歴が真正であること
- amountを持たない`RefundFinalized` eventが、gross全額をmintする検証済みBase operationと一致すること
- IC staging、Base Sepolia、公式EVM RPC Canisterを通した実rehearsal
- production hardware wallet ceremony、controller handover、monitor drill
- OISY、Plug、browser extensionの本番versionとの手動互換matrix

## 検証状態

現行sourceでCanister unit 96件、UI unit 65件、Contract 83件、Lean `sorry`禁止とtypecheck、Solidity SMT pass/fail fixture、Verus 55 obligations / 0 errors、schema/ABI/Candid drift検査が成功した。RPC監査を含む全transaction経路のfailpoint/reopen検査も成功した。

直前の統合実行ではPocketIC 53件、desktop/mobile Playwright 8件、local IC/Anvil smokeも成功した。その後の監査transaction変更を含む現行sourceでは、sandboxのlocal socket制限によりPocketIC/real Playwrightを完走できていない。approval待機中のaccount/chain/signer/code driftはunitで送信未実行を確認したが、desktop/mobile Playwright fixtureは未完了である。RPC監査failpointとfault-injection証跡のblocking findingは解消したが、`codex-review-gate`はこのPlaywright drift fixtureをblocking findingとして残している。このため全CI完了条件とproduction blockerは満たしていない。

`scripts/ci-local.sh all`はRust、PocketIC、Foundry、Leanのproof escape guard、Lean、Verus、SMT、UI typecheck/lint/unit/build、desktop/mobile Playwright、schema/ABI/Candid drift、local ICとAnvil smokeを一括実行する。

外部networkへのbroadcast、実送金、controller handover、unpauseはこの検証で実行していない。

## 受容済みリスク

- Continue quotaは追加しない。所有者による順次実行のcycle消費と、複数principalによるnotification quotaのSybil DoSを受容する。
- 自動retryは追加しない。復旧は明示RefreshまたはContinueを前提とする。
- Withdrawal Historyの古い範囲は自動全走査せず、利用者の`Scan older`操作を要する。
- 単一release approverの誤承認と鍵喪失を受容する。
- hardware walletの物理的独立性と担当者応答は、署名付きattestationと演習証跡に依存する。
- Base Safe確認後からL1 finalityまでのreorgにより、Base上のburnが消えた後もICP releaseが残る可能性を受容する。

## Production deploy blocker

次の実証が揃うまでproduction deployを承認しない。

1. source、Wasm、runtime bytecode、profile、Gate A receiptへbindingされた署名済みGate B bundle
2. profile、Base、Canisterのsigner一致と、公式EVM RPC Canister経由で`COMPLETE`になった実rehearsal
3. proposerとexecutorから分離したcanceller hardware walletを含むceremony証跡
4. 同じT0から計測した5分、15分、60分のmonitor drill証跡
5. Timelock、IC controller、reserve、runtime codeのlive検査成功
6. 現行source hashに対する`scripts/ci-local.sh all`とreview gateのblocking finding 0

providerの運営主体と基盤の独立性はblockerに含めない。
