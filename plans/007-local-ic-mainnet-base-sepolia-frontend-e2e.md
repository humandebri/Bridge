# Plan 007: Local → IC Mainnet × Base Sepolia Frontend E2E

## 状態

- **Local implementation**: DONE
- **Local promotion evidence**: clean commit作成後の再実行待ち
- **IC mainnet / Base Sepolia / Cloudflare Worker**: 明示承認待ち（未実行）
- **Staging evidence tooling**: DONE（ordered manifest、offline verifier、RPC fault injector）
- **Production / SNS**: 対象外

本計画はproduction資産、production Canister、KINIC Ledger、Base Mainnet、SNS controllerを変更しない。外部実行前に、同一commitから生成した`local-e2e.json`を必須とする。

## 固定構成

Plan 007のローカルE2Eでは次の3件を使用する。

- `bridge-sepolia`: `test-deployment` featureを明示した専用recipe
- `ledger-sepolia`: `ledger-suite-icrc-2026-03-09`の固定Wasm
- `index-sepolia`: 同releaseの固定Wasm

IC mainnet上の`sepolia-staging`で新規作成またはinstallするのは`bridge-sepolia`だけとする。test tokenには既存の共有`testicrc` Canisterを再利用し、staging専用LedgerまたはIndex Canisterは新規作成しない。ローカルE2E用の固定Ledger/Index recipeを外部stagingの作成指示として扱ってはならない。既存`testicrc`の実Canister IDとmetadataは外部stage開始前にlive状態から確認する。

test frontendはIC Asset Canisterへ配置しない。完成した`frontend-profile.json`を埋め込んで静的assetをbuildし、Wranglerのtest専用コマンドでCloudflare Worker `kinic-bridge-ui-test`へ公開する。Workerは静的assetだけを配信し、server-side state、database、KV、secretを持たない。

staging Bridgeは公式EVM RPC Canister `7hfb6-caaaa-aaaar-qadga-cai`を使用する。frontend profileは`testOnly: true`、chain ID `84532`、test専用Canister ID、contract address、runtime hashを必須とする。UIは常時TEST bannerを表示し、Base Mainnet、production Canister ID、非公式EVM RPC IDとの混在を拒否する。

## Local promotion gate

```sh
scripts/plan007-local-gate.sh
```

gateはRust、Solidity、Verus、Candid/ABI、UI、ICP buildと、PocketIC・実ICRC Ledger/Index・Anvil・test frontendを接続したPlaywright E2Eを実行する。E2Eは次を実証する。

- pause install後にCanister由来Mint SignerとGovernance Operatorを導出する。
- deployer roleを残さずTimelock、Bridge、bSNSを配置する。
- Canisterの引数なし`schedule_activation()`、早期execute revert、Anvilだけの72時間経過、`execute_activation()`を確認する。
- Deposit、Base mint、Finalized wallet confirmation、Withdrawal、Ledger release、reload、duplicate、二重タブleaseを確認する。
- 同一Wasm upgradeで未完了state、nonce queue、pause、rate limitを保持する。
- raw EVM transactionのhash、RPC返却hash、採掘receiptを一致させる。

一件でも失敗した場合、またはworking treeがdirtyな場合は`deployments/sepolia-staging/evidence/local-e2e.json`を発行しない。証跡はsource commit、Bridge Wasm、contract runtime、Candid、ABI、固定Ledger/Index Wasmのhashを記録する。

外部stageは[`sepolia-staging-e2e.md`](../docs/runbooks/sepolia-staging-e2e.md)に従い、`staging-e2e-driver.sh`で順序と証跡を固定する。最終`sepolia-e2e.json`はlocal evidence、frontend profile、live artifact、wallet matrix、RPC rehearsal、upgrade、final pauseを同じsource commitへ束縛する。

## 承認後の外部stage

外部stageは次の順序を変更しない。

1. `local-e2e.json`とsource commitを再検証する。
2. `sepolia-staging`のBridgeだけを作成または再利用してcyclesを補充する。既存`testicrc`のCanister IDとmetadataを確認し、専用Ledger/Indexは作成しない。
3. Bridgeをpause状態でinstallし、Mint SignerとGovernance Operatorを取得する。
4. test minting accountからTEST KINICを配布する。minting accountへBridge権限は与えない。
5. 一時deployerでBase Sepoliaへ72時間Timelock、Bridge、bSNSを配置する。人間EOA roleがゼロであることを確認する。
6. test Bridgeからactivationをscheduleする。
7. 72時間待機中に、完成した`frontend-profile.json`から静的assetをbuildし、`ui`で`pnpm run deploy:test`を実行してCloudflare Worker `kinic-bridge-ui-test`へ公開する。
8. 72時間後にtest Bridgeからexecuteし、canonical Finalized receipt後のIC Deposit resumeを確認する。
9. OISY、Plug、Base walletで実Deposit/Withdrawal、reload、account/chain変更、pause、upgradeを行い、`sepolia-e2e.json`を作る。
10. IC DepositとBase両flowをpauseし、pending Timelock operationがない状態で終了する。

新規作成または再利用するBridge Canister IDは`.icp/data/mappings/sepolia-staging.ids.json`だけへ保存する。既存`testicrc`は作成対象のmappingへ追加しない。`production.ids.json`は変更しない。ICP操作はICP CLIだけを使用し、`dfx`を使用しない。

各外部stageは、Canister作成・cycles投入、Base Sepolia transaction、Cloudflare Worker公開の直前にそれぞれ明示承認を得る。Gate A/B、鍵ceremony、SNS handoverは作成しない。

## 完了条件

Plan 007の完了は、local evidenceに加えてOISY/Plug双方の実Deposit、双方宛のWithdrawal release、RPC障害、pause/cancel、reload/duplicate、同一Wasm upgradeを`sepolia-e2e.json`へ固定し、test環境をpauseしてpending operationをゼロにした時点とする。100回gas/cycles計測とSNS proposal upgradeはPlan 005/006に残す。x402はBridgeの完了条件に含めない。
