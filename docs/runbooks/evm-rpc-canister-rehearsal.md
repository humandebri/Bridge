# EVM RPC Canister経由Base Sepolia実演習

このrunbookは、公式EVM RPC Canisterを経由するBridgeのtest-only実演習と、その証跡の作成手順を定める。
既存の`base-sepolia-experiment`はcontract-onlyであり、この実演習の証跡には使用しない。

通常CIは外部callやtransactionを実行しない。
CIが確認するのは、rehearsal recorderのテスト、公式Canister IDへの固定、禁止されたローカルtest backendへの参照がないことだけである。

## 保証境界

- networkはIC mainnetとBase Sepolia（chain ID `84532`）に固定する。
- EVM RPC CanisterはDFINITY管理の`7hfb6-caaaa-aaaar-qadga-cai`に固定する。
- custom RPC URLはcredentialを含まないHTTPSを3件指定し、URL文字列の重複だけを拒否する。
- providerの運営主体、upstream、ASN、cloud、region、障害ドメイン、可用性は監査しない。
- 「EVM RPC Canisterと設定providerのquorumがcanonical Finalized Base Sepolia chainを正しく返す」ことは外部仮定として証跡に残す。
- orphan receipt、same-height hash不一致、provider誤応答の決定的検査は既存PocketICテストの責務とする。実公開RPCへの故障注入は本番承認条件にしない。

公式Canister IDとinterfaceの一次資料は、DFINITYの[EVM RPC documentation](https://internetcomputer.org/docs/references/evm-rpc-canister)および[EVM RPC canister repository](https://github.com/dfinity/evm-rpc-canister)を参照する。ネットワーク取得はCIの前提にしない。

## 必要な外部入力

次が一つでも欠ける場合は開始しない。

- IC上のtest Bridge Canister、test ICRC Ledger、test ICRC Index
- 初期pause状態のBase Sepolia専用Bridge
- Bridge Canisterの十分なcycles
- test Ledger残高とBase Sepolia ETH
- chain-key signerと一致するBase Bridge signer
- credentialなしの公開HTTPS RPC URL 3件
- test principalの認証手段
- production候補と同一buildから得たBridge Canister WasmとBridge runtime bytecodeのSHA-256
- 各操作のrequest/responseを秘密除去後にSHA-256化できる記録手段

本番鍵、seed、private key、hardware wallet backup、password、credential付きURL、生のauthorization headerをconfig・証跡・shell引数へ保存しない。

## 初期化とpreflight

templateを作業ディレクトリへコピーし、全placeholderを実test値へ置換する。

```sh
cp deployments/evidence-templates/evm-rpc-rehearsal-config.template.json \
  /secure/work/rehearsal-config.json
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  validate-config /secure/work/rehearsal-config.json
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  init /secure/work/rehearsal-config.json /secure/work/rpc-e2e.json
```

`validate-config`は、IC network、Base Sepolia、公式EVM RPC Canister、3件のsecret-free HTTPS URL、test-only binding、Bridge Canister WasmとBridge runtime bytecodeのSHA-256を検査する。
出力はURLをhostとSHA-256へ縮約し、完全URLをmanifestへ保存しない。

外部callを行う前に、Bridge Canisterのchain/canister/contract/RPC設定、chain-key signer、同じFinalized Base blockのBridge signer、両方向pause、cyclesとtest ETHをlive状態から再読する。

preflight evidenceの`details`は次の完全なfield集合とする。

```json
{
  "observed_chain_id": 84532,
  "observed_evm_rpc_canister_id": "7hfb6-caaaa-aaaar-qadga-cai",
  "observed_bridge_contract": "0x...",
  "base_bridge_signer": "0x...",
  "canister_chain_key_signer": "0x...",
  "deposits_paused": true,
  "withdrawals_paused": true,
  "cycles_balance": 10000000000,
  "base_sepolia_eth_balance_wei": 1,
  "configured_rpc_url_sha256": ["...", "...", "..."]
}
```

観測値を手入力する前に、固定driverで`dfx`と`cast`のJSON出力をraw artifactへ保存する。
driverはshellを介さずcommandを実行し、argv、exit status、raw stdout、stdout digest、JSON parse結果を一つのartifactへ保存する。

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py capture-artifact \
  /secure/work/rpc-e2e.json /secure/work/rehearsal-config.json preflight bridge \
  /secure/work/artifacts/preflight-bridge.json none -- \
  dfx canister call <bridge-canister-id> get_public_config '()' \
  --network ic --output json

python3 scripts/evm-rpc-rehearsal/rehearsal.py capture-artifact \
  /secure/work/rpc-e2e.json /secure/work/rehearsal-config.json canonical_receipt base \
  /secure/work/artifacts/canonical-receipt-base.json 0 -- \
  cast receipt <transaction-hash>
```

`bridge`と`audit` artifactはtest Bridge Canister、`ledger`はconfigのtest Ledger、`base`はBase Sepoliaのreceipt/block/callだけを許可する。
local backend、test double、別Canister、別network、JSON以外の出力、失敗commandは拒否する。
Base captureのendpointはreview済みconfigの`rpc_urls[provider-index]`からだけ選択し、driverが同endpointへ`cast chain-id`を実行して`84532`を確認してから本callを行う。
artifactには完全URLを残さずprovider indexとURL SHA-256、chain-id応答、method、paramsを残す。`ETH_RPC_URL`等の環境override、command内の`--rpc-url`、`--chain`、`--json`、重複network flagは拒否する。
Bridge状態取得は実Candid名`get_bridge_status`を使用し、旧`get_status`は拒否する。

故障scenarioは手書きJSONを使わず、固定名`evm-rpc-fault-injector`をPATH上に用意し、review済み設定をstdinで受けて故障適用・scenario実行・全provider復旧後にJSON結果を返す。recorderは対象URLのdigest、故障provider index、failure rule、実行区間、injector出力を決定的に束縛する。

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py capture-fault \
  /secure/work/rpc-e2e.json /secure/work/rehearsal-config.json single_provider_failure \
  /secure/work/artifacts/single-provider-fault.json fault-one-provider -- \
  evm-rpc-fault-injector
```

injectorは`schema_version`、`run_reference`、`applied_provider_indices`、`restored_provider_indices`、`result: "completed"`に加え、故障区間中に取得した`decision_sequence`、`decision_timestamp_ns`、完全な`canister_decision`をJSONで返す。recorderはdecisionのcanonical digestを保存し、validatorはscenarioのdecisionと一致し、そのtimestampが故障適用から復旧までの区間内にある場合だけ受理する。review済みindexと不一致、復旧未確認、別時点のdecision、失敗exit、任意引数付きinjectorは拒否する。

scenario evidenceの`artifacts`へartifactの相対path、ファイル全体のSHA-256、`details`各fieldをraw stdoutへ結ぶJSON pointerを記載する。
全detail fieldがraw artifactから再導出できなければ`verify`は失敗する。ID、Ledger block、transaction、canonical hash、quorum、nonce結果はscenarioごとにBridge/Base/Ledger/auditの複数artifactへcross-bindingする必要があり、一種類の自己申告だけでは完了しない。
`request_sha256`はartifact順の`[tool, argv..., transport]`配列、`response_sha256`は同順のraw stdout配列を空白なしJSONへしたSHA-256として算出する。任意hashは受理しない。

観測結果、artifact binding、秘密除去済みrequest/responseのdigestをtemplateへ入れ、次で記録する。

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  record /secure/work/rpc-e2e.json preflight /secure/work/preflight.json
```

`external_calls_performed=true`と`through_evm_rpc_canister=true`だけでは証跡にならない。
quorum成功scenarioは`get_audit_events`の`EvmRpcObservation`から、EVM RPC Canister ID、Candid call method、Canister内部request digest、quorum response digest、Finalized block number/hash、transaction hashを`canister_audit`へ束縛する。
call methodはscenarioに応じたproduction実値`multi_request`、`eth_sendRawTransaction`、`eth_sendRawTransaction+multi_request`、`eth_getTransactionReceipt+eth_getBlockByNumber`だけを許可する。`nonce_conflict`もbroadcast decision auditを必須とし、単なるstop reason自己申告では完了しない。
`single_provider_failure`、`quorum_loss`、`nonce_conflict`は`EvmRpcDecision`も`canister_decision`へ束縛し、設定provider数、必要threshold、停止理由、Ledger呼出し有無、Bridge継続、Deposit pause、自動再署名有無を再導出する。threshold APIは採用前のprovider別全responseを返さないため、3/3一致と2/3一致の区別はfault injection artifactへ委ね、Canister auditは設定値`3`、必要threshold`2`、実際の継続・停止判断を証明する。
`preflight`ではさらに固定`dfx canister status` captureのmodule hashをreview済みWasm SHA-256へ束縛する。
予定値、手入力digest、dry-runを証跡として記録してはならない。

## 段階実行

state machineは次の順に進む。完了済みscenarioへ異なる証跡を上書きできない。

```text
AWAITING_PREFLIGHT
  -> READY_FOR_ASSET_FLOWS
  -> READY_FOR_FAILURE_SCENARIOS
  -> READY_FOR_FINAL_PAUSE
  -> AWAITING_RAW_ARTIFACT_VERIFICATION
  -> COMPLETE
```

asset flowとして次の4件を実行し、各transactionをFinalized headまで待つ。

1. `deposit_mint`: Deposit ID、Ledger block、mint transaction、Safe block/hash
2. `withdrawal_release`: user `approve`、user `createWithdrawal`のFinalized block/hash、固定quote、ICRC transfer block、追加Base transactionがないこと
3. `bad_fee_hold`: Ledger FeeがService Feeを超えた場合の送金前停止と、同じIC Accountの維持
4. `canonical_receipt`: receipt block number/hash、同heightのcanonical hash、Finalized head

failure scenarioとして次の4件をtest-only設定で実行する。

1. `single_provider_failure`: configured provider 3、required threshold 2、1 provider故障注入のraw参照、threshold成立、Bridge処理継続
2. `quorum_loss`: required threshold 2、2 provider以上の故障注入、threshold不成立、`RpcInconsistent`または`RpcUnavailable`、Ledger call前fail-closed
3. `nonce_known`: `NonceTooLow`後、local transaction hashが2-provider合意で存在し`Submitted`
4. `nonce_conflict`: local transaction hash不在、`NonceConflict`、自動再署名なし、Deposit pause

failure用endpointへの一時差替えはtest Bridge Canisterだけで行い、通常3 endpointを使う正常系証跡と混在させない。
一時設定、操作時刻、元設定への復旧を別の運用ログへ残す。
EVM RPC clientはthreshold判定に使ったprovider別全responseやexact agreeing countを公開しないため、`agreeing_provider_count`は証跡にしない。
代わりにconfigured count、required threshold、故障注入artifact、処理継続またはfail-closed decisionをthreshold certificateとして記録する。
故障注入条件はBridge/Canister auditへ存在しないfieldを合成せず、専用`fault` raw artifactへ分離する。このartifactは`rehearsal_id`、scenario、run reference、configured provider count 3、required threshold 2、failed provider count、request/config digestを持ち、manifest hashで保護する。Canister `EvmRpcDecision`は継続またはfail-closedの判断だけを証明する。

failure scenario後に`final_pause`を記録する。BaseのDeposit/WithdrawalとCanisterの新規Deposit受付をpauseし、Base側pause transactionのFinalized block/hashを再読する。

各scenarioの`details`の正確なfield名と型は`scripts/evm-rpc-rehearsal/rehearsal.py`がfail closedで検査する。
templateの`details`文字列を実objectへ置換し、次のように一件ずつrecordする。

```sh
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  record /secure/work/rpc-e2e.json deposit_mint /secure/work/deposit-mint.json
python3 scripts/evm-rpc-rehearsal/rehearsal.py \
  verify /secure/work/rpc-e2e.json
```

## 終了条件

- manifestが`COMPLETE`かつ`complete=true`である。
- 全10 scenarioが公式EVM RPC Canister、Base Sepolia、同じrehearsal ID、同じBridge Canisterへbindingされている。
- rehearsalのsource revision/tree、Bridge Canister Wasm、Bridge runtime bytecodeがrelease bundleと一致する。
- quorum成功scenarioの`canister_audit`がraw `get_audit_events` artifactから再導出され、preflight module hashがreleaseのBridge Wasmと一致する。
- signer triple、receipt/canonical block hash、confirmationが一致する。
- quorum lossはLedger call前に停止する。
- unknown nonce conflictは自動再署名せずDepositをpauseする。
- Base BridgeとCanisterは演習終了時もpause状態に戻す。資産受付開始はこの演習とは別の明示承認とする。
- `rpc-e2e.json`のSHA-256をrelease evidence bundleへ登録し、参照する`artifacts/`も同じbundleへ含める。artifactにはcredentialを含まないraw command stdoutだけを保存し、生authorization、credential URL、秘密は含めない。

`COMPLETE`は実演習の証跡が構造上揃ったことだけを意味し、本番deploy、controller handover、unpause、資産受付開始を承認しない。
