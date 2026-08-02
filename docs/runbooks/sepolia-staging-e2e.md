# IC mainnet × Base Sepolia staging E2E

このrunbookはPlan 007の外部stageを、同一source commitへ束縛された再開可能な証跡として実行する。
production Canister、KINIC Ledger、Base Mainnet、SNSを対象にしてはならない。

Base Sepolia stagingだけは`short-delay-test-only` policyによりactivation delayを300秒とする。production artifactの24時間制約は変更せず、短縮版artifactと証跡をproduction rehearsalへ使用しない。

## Test Ledgerのfee

このstagingはKINICではなく、共有test tokenのTICRC1を使用する。
TICRC1 Ledgerのfeeは`10000` rawであり、KINIC mainnet Ledgerの`100000` rawとは異なる。

現在のstaging Wasmに含まれる`KINIC_LEDGER_FEE = 10000`は、TICRC1へ合わせたtest-only値である。
この差は設定driftではなく、stagingとproductionで対象Ledgerが異なるために意図して設けている。

stagingの検証では、Canisterの`get_public_config().ledger_fee`、TICRC1 Ledgerの`icrc1_fee()`、staging profileの値がすべて`10000` rawで一致することを確認する。
production artifactへこの値を流用せず、KINIC mainnet Ledgerのlive feeと承認済みproduction profileへ定数を同期した別commitからWasmを作成する。

## 開始条件

- clean commitで`scripts/plan007-local-gate.sh`を実行し、現行commitの`local-e2e.json`を発行する。
- `frontend-profile.json`の値を予定値として信用せず、共有test Ledger/Index、staging Bridge、Base Sepoliaのlive値を再読する。
- ICP identity名、wallet secret、RPC fault controller tokenはリポジトリへ保存しない。
- Canister install/reinstallとcycles投入、Base Sepolia transaction、Cloudflare test UI公開は別々に承認を得る。

## 証跡state machine

次でmanifestを初期化する。現行commitからgateが生成した`local-e2e.json`の差分だけは許可するが、
それ以外のdirty treeまたは古いlocal evidenceでは失敗する。

```sh
scripts/plan007/staging-e2e-driver.sh init
scripts/plan007/staging-e2e-driver.sh status
```

stageは次の順序に固定され、途中を飛ばせない。

```text
preflight
  -> contracts
  -> install
  -> initialize
  -> activation_schedule
  -> activation_execute
  -> frontend_publish
  -> smoke_e2e
  -> wallet_e2e
  -> rpc_rehearsal
  -> final_pause
  -> COMPLETE
```

各stageは、操作後に取得したraw artifact、artifact SHA-256、source commit、観測値だけをstage evidenceへ記録する。
予定値、手入力した成功要約、失敗commandの出力をPASS証跡にしない。

`reinstall` の前には、live `public_config` をJSONへ保存し、次のgateを必ず通す。
現行v31のreinstallではprofileの新IDがlive IDと異なることを要求し、欠落、ゼロ値、
同一ID、旧・未知schemaをfail closedにする。
出力の `live_schema_version` と `previous_deployment_instance_id` をpreflight証跡へ転記し、
manifest検証でも同じ比較を行う。

```sh
scripts/plan007/staging-e2e-driver.sh check-reinstall-instance \
  /secure/work/live-public-config.json \
  > /secure/work/reinstall-instance-check.json
```

checkerへ渡した `live-public-config.json` と、その標準出力
`reinstall-instance-check.json` を変更せずmanifestディレクトリ配下へコピーする。
preflight evidenceの`artifacts`では、それぞれ一意なkind `live-public-config` と
`reinstall-instance-check` を付け、コピー後のSHA-256を記録する。manifest validatorは
両artifactを再読し、live設定から比較を再計算してchecker出力および`details`と照合する。
別のlive取得結果、手編集したchecker出力、manifest外のpathは使用しない。

```sh
scripts/plan007/staging-e2e-driver.sh record /secure/work/preflight-evidence.json
scripts/plan007/staging-e2e-driver.sh status
```

manifestとartifactのfield集合、hash、stage順序、ネットワーク固定、wallet matrix、upgrade、最終pauseは
`scripts/plan007/sepolia_e2e.py`がfail closedで検査する。

## RPC故障演習

通常経路は公式EVM RPC Canisterとcredential-free HTTPS RPC 3件を使う。
故障演習だけは、復旧制御APIを持つtest-only proxy 3件をsecure configへ設定する。
control tokenは`BRIDGE_E2E_FAULT_CONTROL_TOKEN`、config pathは`BRIDGE_E2E_FAULT_CONFIG`で渡す。

```sh
export BRIDGE_E2E_FAULT_CONFIG=/secure/work/evm-rpc-fault-config.json
export BRIDGE_E2E_FAULT_CONTROL_TOKEN='<ephemeral-token>'
scripts/plan007/staging-e2e-driver.sh rpc-capture-fault \
  /secure/work/rpc-e2e.json \
  /secure/work/rehearsal-config.json \
  single_provider_failure \
  /secure/work/artifacts/single-provider-failure.json \
  single-provider-failure-1
```

injectorは1-providerまたは2-providerの固定failure setだけを受理し、Bridge callとaudit取得後に全providerを必ず復旧する。
部分適用、Bridge call失敗、audit不足、復旧失敗はすべてscenario失敗とする。

## Wallet E2E

公開test UIと実Chromeを使い、`ui-wallet-compatibility.md`を完了する。
Plug/OISYのDeposit、MetaMask/RabbyのWithdrawal、WalletConnect、reject、popup close、reload、
duplicate/conflict/sequence gap、二重tab、account/chain変更、runtime mismatch、通知復旧を記録する。
同一Wasm upgradeでは、前後のcanonical state SHA-256一致と`storage_integrity_check() = "ok"`を必須とする。

## 終了条件

Base Deposit、Base Withdrawal、IC Depositをpauseし、pending Deposit、Withdrawal、Timelock operationをゼロにする。
fault proxy 3件の通常応答を再確認してから`final_pause`を記録し、完全検証を実行する。

```sh
scripts/plan007/staging-e2e-driver.sh verify
```

`COMPLETE`はtest-only staging E2Eの完了だけを意味し、本番deploy、SNS操作、資産受付開始を承認しない。
