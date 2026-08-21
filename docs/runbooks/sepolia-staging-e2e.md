# IC mainnet × Base Sepolia staging E2E

このrunbookはPlan 007の非blocking外部stageを、schema v7で同一source commitへ束縛された再開可能な証跡として実行する。
production Canister、KINIC Ledger、Base Mainnet、SNSを対象にしてはならない。

Base Sepolia stagingだけは`short-delay-test-only` policyによりactivation delayを300秒とする。production artifactの24時間制約は変更せず、短縮版artifactと証跡をproduction rehearsalへ使用しない。

## Test Ledgerのfee

このstagingはKINICではなく、共有test tokenのTICRC1を使用する。
TICRC1 Ledgerのfeeは`10000` rawである。

`test-deployment` featureで作るstaging Wasmは`KINIC_LEDGER_FEE = 10000`を使用し、production Wasmは`100000`を使用する。
この差は設定driftではなく、stagingとproductionで対象Ledgerが異なるために意図して設けている。staging artifactをproductionへ流用してはならない。

stagingの検証では、Canisterの`get_public_config().ledger_fee`とTICRC1 Ledgerの`icrc1_fee()`がともに`10000` rawで一致することを確認する。
production artifactではKINIC mainnet Ledgerのlive fee、固定値`100000`、承認済みproduction profileが一致することを別途検証する。

## 開始条件

- clean commitで`scripts/plan007-local-gate.sh`を実行し、現行commitの`local-e2e.json`を発行する。
- `frontend-profile.json`の値を予定値として信用せず、共有test Ledger/Index、staging Bridge、Base Sepoliaのlive値を再読する。
- ICP identity名、wallet secret、RPC fault controller tokenはリポジトリへ保存しない。
- Canisterの初回installと同一instance upgrade、cycles投入、Base Sepolia transaction、Cloudflare test UI公開は別々に承認を得る。初期化済みCanisterのreinstallは行わない。
- install前にIC Deposit、Base Deposit Mint、Base Withdrawalをpauseし、Finalized postconditionを記録する。どれか一つでもpauseできない場合は後続を実行しない。

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
  -> SHORT_DELAY_COMPLETE
```

各stageは、操作後に取得したraw artifact、artifact SHA-256、source commit、観測値だけをstage evidenceへ記録する。
予定値、手入力した成功要約、失敗commandの出力をPASS証跡にしない。

Canister upgradeの前にはlive `public_config` をJSONへ保存し、次のgateを必ず通す。
review済みv34の同一instanceからv35へのupgradeだけを`current-schema-upgrade`として受理し、upgrade前後のstate count、
source schema v34、target schema v35、instance ID、`storage_integrity_check = ok`を照合する。異なるinstance、reinstall、v33以下、
未知schema、欠落、ゼロ値はfail closedにする。新規Canisterへの初回installだけはこのupgrade gateの対象外である。
出力の `live_schema_version` と `previous_deployment_instance_id` をpreflight証跡へ転記し、manifest検証でも同じ比較を行う。

```sh
scripts/plan007/staging-e2e-driver.sh check-upgrade-instance \
  /secure/work/live-public-config.json \
  /secure/work/live-canister-status.json \
  > /secure/work/upgrade-instance-check.json
```

checkerへ渡した `live-public-config.json`、`live-canister-status.json` と、その標準出力
`upgrade-instance-check.json` を変更せずmanifestディレクトリ配下へコピーする。
preflight evidenceの`artifacts`では、それぞれ一意なkind `live-public-config`、`live-canister-status` と
`upgrade-instance-check` を付け、コピー後のSHA-256を記録する。manifest validatorは
各artifactを再読し、live設定とmodule hashから比較を再計算してchecker出力および`details`と照合する。
別のlive取得結果、手編集したchecker出力、manifest外のpathは使用しない。

履歴を失ったstate、v33以下、旧wireのstaging canisterはupgradeせず、現行Wasmを新規Canister IDへinstallして検証stateを作り直す。

v34→v35 upgradeではさらに、同じ観測時点の次のJSON artifactを保存する。

- `live-bridge-status`: Deposit／reservationを含むcountsと、Withdrawal、pending Ledger operation、reconciliation hold、未払額を保持する。
- `live-activation-status`: pending Timelock operation数を保持する。
- `live-canister-status`: module hash、controller principals、cycles balanceを保持する。
- `live-storage-integrity`: 認可済みcallerによる`storage_integrity_check()`の`ok`結果を保持する。
- `live-ledger-balance`: Bridge principalのTICRC1 raw balanceを保持する。
- install stageにはupgrade前後の全count、source schema v34、target schema v35、同一instance ID、`storage_integrity_check = ok`を記録し、いずれかが不一致なら後続activationへ進まない。

manifest validatorは全artifactを再hashし、snapshot間のcount、module hash、balance、instance IDを再比較する。pending Timelock operationはupgrade前にゼロでなければならない。Deposit、reservation、Withdrawal、pending Ledger operation、hold、監査履歴は同一schema upgrade後も保持する。

`test-deployment` Wasmに限り、IC HTTPS outcallで401を返すOnFinalityをdRPCへ置換する今回専用のupgrade引数を受理する。引数なしの`()`はRPC設定を変更しない。更新指定は次のCandidに固定し、PublicNode、`sepolia.base.org`、dRPCの順序を変えない。

```candid
(
  record {
    status_counts_guard_version = 1 : nat8;
    rpc_provider_update = opt record {
      custom_evm_rpc_urls = vec {
        "https://base-sepolia-rpc.publicnode.com";
        "https://sepolia.base.org";
        "https://base-sepolia.drpc.org";
      };
      expected_status_counts = record {
        retained_audit_events = 15 : nat64;
        reconciliation_holds = 0 : nat64;
        retained_deposit_index_entries = 1 : nat64;
        pending_ledger_operations = 0 : nat64;
        withdrawals = 1 : nat64;
        deposits = 1 : nat64;
        reserved_deposit_mint_operations = 1 : nat64;
        reserved_deposit_mint_amount = 1_050_000_000 : nat;
        pruned_audit_events = 0 : nat64;
      };
    };
  },
)
```

この更新は旧digest `3ab53c0532b80b3f39ed076f9661794c0a847b0d2eba1845b5c7e0ed1663ed48`から新digest `df7e867aaf6abeaf00b0f61e8662fa87c6f8675eb0aebdf7b09f8c99a499d064`だけを許可する。新設定の同値再実行は成功するが、URL・順序・chain ID・EVM RPC Canister ID・現在digestの不一致はtrapしてupgrade全体をrollbackする。更新後もFinalized水位を保持し、runtime attestation cacheだけを無効化する。production Wasmの`post_upgrade()`は引数なしで、この更新経路を含まない。

PR #11の新profileを含むclean checkoutから、repository-owned
`rpc-provider-replacement-policy.json`に固定したCanister ID、source schema v34、target schema v35、instance ID、変更前module hash、
state count、旧・新RPC順序とdigestをreviewする。driverは`local-e2e.json`のsource commitとWasm／Candid hash、
live Candid互換性、認証済み`storage_integrity_check`、固定順序3 endpointのchain IDを照合する。
通常実行はread-only preflightだけを行い、`--execute`を追加した別承認の呼出しだけが明示Wasmをinstallする。
到達不能、不正応答、binding不一致、state driftはRPC置換の適用前にfail closedとする。
Python driverのsnapshot照合は、operatorへ早期にdriftを示す事前診断である。`--execute`では同じreview済み
state countと固定guard version `1`をupgrade引数にも含め、test-deployment Wasmの`post_upgrade()`がstable stateを再openした直後、
設定またはruntime attestationを書き換える前に全countを再照合する。preflight後にstateが変わった場合は
`post_upgrade()`がtrapし、RPC置換を含むupgrade全体をrollbackする。

```sh
BRIDGE_STAGING_IDENTITY=<identity> \
  scripts/plan007/staging-canister-upgrade.sh \
    --wasm "$(pwd)/target/test-deployment/staging/bridge_canister.wasm" \
    --evidence /secure/work/staging-rpc-replacement.json

# preflight結果とpolicyを別レビューし、Canister upgradeの明示承認後だけ実行する。
BRIDGE_STAGING_IDENTITY=<identity> \
  scripts/plan007/staging-canister-upgrade.sh --execute \
    --wasm "$(pwd)/target/test-deployment/staging/bridge_canister.wasm" \
    --evidence /secure/work/staging-rpc-replacement.json
```

driverは`icp deploy`や暗黙buildを使用しない。成功または適用済みpostconditionをすべて確認した場合だけ
evidenceをatomicに確定する。install失敗またはpostcondition不一致では再実行せず、CLI出力と未確定artifactを
保全して原因をreviewする。過去のarchive evidenceは変更しない。

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

`SHORT_DELAY_COMPLETE`はtest-only stagingの追加wallet互換性と全10 RPC scenarioを含む詳細E2Eの完了だけを意味し、本番deploy、SNS操作、資産受付開始を承認しない。この詳細完了はproduction activationをblockしない。
