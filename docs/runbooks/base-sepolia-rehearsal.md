# Base Sepolia contract実験

このrunbookは、Base Sepolia上でTimelock、Bridge、bSNSのcontract-only実験を再開または再実行する手順を定める。
実際のtransaction送信とmanifest更新は`scripts/base-sepolia-experiment/`のstate machineを使用する。
IC canisterとKINIC Ledgerは接続しない。

## 実験の境界

- networkはBase Sepolia、chain IDは`84532`とする。
- 公開RPCの初期値は`https://base-sepolia-rpc.publicnode.com`とする。
- RPCは実行時にchain IDを検査するため、別providerへ差し替えられる。
- credentialを含むRPC URLは公開manifestへ保存される可能性があるため使用しない。
- 実験walletはtest-onlyとし、本番鍵や本番ceremonyへ再利用しない。
- 今回のdeployerはBase Admin walletとRuntime Administratorを兼ねる。
- Bridge signerは別walletとする。
- この構成は本番profileのrole分離を満たさない。

## 公開値

- **Deployer、Base Admin、Runtime Administrator**：`0x7F4743128368CdeD5413E8c42C9Bd689ea64D192`
- **Bridge signer**：`0xF96808b465638E88Ed4602b3852Ce7AC92E57721`
- **Timelock delay**：`259200`秒（72時間）
- **Per-Deposit Limit**：`1000000000` raw
- **Mint Window Limit**：`10000000000` raw
- **Mint Window Duration**：`3600`秒
- **MAX_SERVICE_FEE**：`10000000` raw
- **Initial Service Fee**：`1000000` raw

上記は2026年7月13日にデプロイ済みの旧実験値であり、証跡として変更しない。
次回の再デプロイでは **Per-Deposit Limit**と**Mint Window Limit**をそれぞれ`15000000000000` raw（150,000 KINIC、総供給量の約2.5%）、**MAX_SERVICE_FEE**を`1000000000` raw（10 KINIC）、**Initial Service Fee**を`50000000` raw（0.5 KINIC）とする。

2026年7月13日のpreflight観測では、chain IDは`84532`、deployer残高は`99000000000000000` wei、nonceは`0`だった。
観測blockと時刻は日付別manifestに保存する。

## 鍵の準備

private key、seed、keystore passwordをリポジトリ、shell引数、shell historyへ保存しない。
Foundryの暗号化keystoreへ対話入力し、keystore passwordはmacOS Keychainへ対話入力する。

```sh
cast wallet import kinic-base-sepolia-experiment --interactive
cast wallet import kinic-base-sepolia-bridge-signer --interactive

security add-generic-password -U \
  -a "$USER" \
  -s kinic-base-sepolia-experiment-keystore \
  -w

security add-generic-password -U \
  -a "$USER" \
  -s kinic-base-sepolia-bridge-signer-keystore \
  -w
```

`-w`は最後の引数に置く。
`security`が対話入力した値を非表示でKeychainへ保存する。

## 実行stage

各stageは現在のmanifest stateを検査し、完了済みtransactionを再送しない。
署名が必要なstageはKeychain wrapperから実行する。

```sh
scripts/base-sepolia-experiment/run-with-keychain.sh preflight
scripts/base-sepolia-experiment/run-with-keychain.sh deploy
scripts/base-sepolia-experiment/run-with-keychain.sh flow
scripts/base-sepolia-experiment/run-with-keychain.sh schedule
```

stageは次の順で進む。

```text
PREFLIGHT
  -> READY_TO_DEPLOY
  -> DEPLOYED
  -> FLOW_COMPLETE
  -> WAITING_TIMELOCK
  -> COMPLETE
```

`preflight`はchain ID、wallet address、Foundry test、ABI drift、固定limit selectorの不存在、deploy gas、最大実験費用を確認する。
推定最大費用が`0.02 ETH`を超える場合はbroadcastしない。

`deploy`はBridge signerへのtest ETH送金、72時間Timelock、Bridgeの順にdeployする。
Bridgeはconstructor内でbSNSを生成する。
各transactionはFinalized block到達まで確認し、30分以内に確認できなければ同じnonceの代替transactionを送らず停止する。

`flow`はDeposit mint、Withdrawal作成、Service Fee変更、DepositとWithdrawalのpauseを実行する。Withdrawal後の追加Base transactionは存在しないことも確認する。

`schedule`はDepositとWithdrawalのunpauseをTimelockへbatch scheduleする。
直後のexecuteを実transactionとして送信し、revert receiptと72時間delayを確認する。

## 72時間後の再開

manifestの`timelock_operation.ready_timestamp`以降に`resume`を実行する。
`resume`はschedule済みpayloadを変更せずexecuteし、unpauseを確認した後、両方向を再びpauseしてService Feeを初期値へ戻す。

```sh
jq '.timelock_operation.ready_timestamp' deployments/base-sepolia-contract-experiment.json
scripts/base-sepolia-experiment/run-with-keychain.sh resume
scripts/base-sepolia-experiment/experiment.sh verify
```

`verify`はread-onlyであり、contract code、role、固定limit、asset state、全receipt、Finalized block、最終pauseをRPCから再読する。

## manifestの扱い

`deployments/base-sepolia-contract-experiment.json`は実行中state machineの作業用manifestである。
スクリプトがaddress、nonce、transaction hash、receipt block、confirmation、runtime bytecode hash、check結果を更新する。

日付別の公開記録は`deployments/base-sepolia/YYYY-MM-DD/manifest.json`へ保存する。
未実行項目は`pending`とし、addressやtransaction hashを推測で埋めない。
実験完了時は作業用manifestの検証済み公開値を日付別manifestへ反映する。

次の情報はどちらのmanifestにも保存しない。

- private keyとseed
- keystore passwordとpassword file
- hardware wallet backup
- credential付きRPC URL
- shell環境の秘密値

## 終了条件

実験終了時はBridgeがtest-onlyであることをmanifestへ残す。
2026年7月13日の旧実験では、Deposit mintとWithdrawalはpause状態、Service Feeは`1000000` rawとする。
次回の再デプロイでは初期Service Feeを`50000000` rawとする。asset-flow試験では管理者変更を確認し、完了前に`50000000` rawへ戻す。
Timelock、Bridge、bSNSのaddressとruntime bytecode hash、全transactionのconfirmationを`verify`で再確認する。
