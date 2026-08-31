# Plan 007: Local → IC Mainnet × Base Sepolia Frontend E2E

## 状態

- **Local implementation**: DONE
- **Local promotion evidence**: clean commit作成後の再実行待ち
- **IC mainnet / Base Sepolia / Cloudflare Worker**: 明示承認待ち（未実行）
- **Staging evidence tooling**: DONE（ordered manifest、offline verifier、RPC fault injector）
- **Production / SNS**: 対象外
- **Production activation dependency**: なし。wallet matrixと追加5 RPC scenarioは非blocking検証として継続する。

本計画はproduction資産、production Canister、KINIC Ledger、Base Mainnet、SNS controllerを変更しない。外部実行前に、同一commitから生成した`local-e2e.json`を必須とする。

## 固定構成

Plan 007のローカルE2EはPocketIC上にBridge、Ledger、Indexを動的作成する。Bridgeは`test-deployment` featureでbuildし、LedgerとIndexには`ledger-suite-icrc-2026-03-09`のchecksum固定Wasmを直接installする。ローカルCanister IDは実行ごとに生成し、`icp.yaml`のenvironmentやmainnet mappingへ保存しない。

IC mainnet上の`sepolia-staging`は、既存`bridge-sepolia` Canister、deployment instance、Base contracts、signer、共有`testicrc` Ledger/Indexをactive stackとして維持する。2026-08-27/28に完了したdestructive reinstallとfresh-stack構築は一度限りの監査履歴へhash固定し、再実行、resume、別stack作成の入力にしない。今後のBridge更新はstable schema v35／record wire v30を保つsame-instance current-schema `upgrade`だけを許可する。

test frontendはIC Asset Canisterへ配置しない。完成した`frontend-profile.json`を埋め込んで静的assetをbuildし、Wranglerのtest専用コマンドでCloudflare Worker `kinic-bridge-ui-test`へ公開する。Workerは静的assetだけを配信し、server-side state、database、KV、secretを持たない。

staging Bridgeは公式EVM RPC Canister `7hfb6-caaaa-aaaar-qadga-cai`を使用する。frontend profileは`testOnly: true`、`environmentMode: short-delay-test-only`、chain ID `84532`、activation delay 300秒、test専用Canister ID、contract address、runtime hashを必須とする。UIは常時TEST・5分Timelock bannerを表示し、Base Mainnet、production Canister ID、非公式EVM RPC IDとの混在を拒否する。

## Local promotion gate

```sh
scripts/plan007-local-gate.sh /secure/work/local-e2e.json
```

gateはRust、Solidity、Verus、Candid/ABI、UI、ICP buildと、PocketIC・実ICRC Ledger/Index・Anvil・test frontendを接続したPlaywright E2Eを実行する。E2Eは次を実証する。

- pause install後にCanister由来Mint SignerとGovernance Operatorを導出する。
- deployer roleを残さずTimelock、Bridge、bSNSを配置する。
- Canisterの引数なし`schedule_activation()`、早期execute revert、staging profileの300秒経過、`execute_activation()`を確認する。default production profileでは別途24時間制約を検証する。
- Deposit、EIP-712 AuthorizationによるBase mint、期限後のFinalized照合、Withdrawal、Ledger release、reload、duplicate、二重タブleaseを確認する。
- 同一Wasm upgradeで未完了state、nonce queue、pause、rate limitを保持する。
- raw EVM transactionのhash、RPC返却hash、採掘receiptを一致させる。

一件でも失敗した場合、またはworking treeがdirtyな場合はlocal promotion evidenceを発行しない。成功時もschema v8 evidenceはrepository外の明示パスへだけ発行し、checked-in v7 `local-e2e.json`を更新しない。証跡はsource commit、Bridge Wasm、contract runtime、Candid、ABI、固定Ledger/Index Wasmのhashを記録する。

外部stageは[`sepolia-staging-e2e.md`](../docs/runbooks/sepolia-staging-e2e.md)に従い、`staging-e2e-driver.sh`で順序と証跡を固定する。driverは検証と記録だけを行い、Canister upgrade、frontend publish、Base transactionを実行しない。最終schema v8 manifestはlocal evidence、frontend profile、live artifact、wallet matrix、refund/RPC rehearsal、same-instance upgrade、reactivation receipt、監視receiptを同じsource commitへ束縛する。

## 承認後の外部stage

外部stageは次の順序を変更しない。

1. `bootstrap_attestation`: 一度限りのreinstall/fresh-stack履歴をhash固定し、再開不能であることを確認する。
2. `preflight`: repository外のschema v8 local evidence、clean source、active Canister/instance、共有Ledger/Index metadata、unpaused状態、storage、固定RPC bindingを再検証する。
3. `current_schema_upgrade`: 別途承認済みtoolingで同じCanisterへcurrent-schema `upgrade`だけを適用し、identityと全state countが不変なreceiptを保存する。
4. `post_upgrade_binding`: module/Candid、schema/wire、deployment instance、minimum Withdrawal ID、contract/runtime bindingをlive再取得する。
5. `frontend_publish`: 別途承認後、同じprofile hashの静的assetを`kinic-bridge-ui-test`へ公開してreceiptを保存する。
6. `smoke_e2e`: unpausedな資産flowでreview済みDeposit/Withdrawalを実行する。
7. `wallet_e2e`: OISY、Plug、MetaMask、Rabby、WalletConnectの成功・失敗経路とreload/account/chain変更を記録する。
8. `refund_rehearsal`: finalized deadline境界とexact未処理証拠によるrefundを記録する。
9. `rpc_rehearsal`: 独立schemaの全10 scenarioをraw artifact付きで`EXTENDED_COMPLETE`まで検証する。
10. `live_acceptance`: 別operationの300秒reactivation schedule/execute receiptと監視receiptを検証し、全pending/liabilityが0かつ資産flowがunpausedの`SHORT_DELAY_LIVE`で終了する。

新規作成または再利用するBridge Canister IDは`.icp/data/mappings/sepolia-staging.ids.json`だけへ保存する。既存`testicrc`は作成対象のmappingへ追加しない。`production.ids.json`は変更しない。ICP操作はICP CLIだけを使用し、`dfx`を使用しない。

各外部stageは、Canister作成・cycles投入、Base Sepolia transaction、Cloudflare Worker公開の直前にそれぞれ明示承認を得る。Gate A/B、鍵ceremony、SNS handoverは作成しない。

## 完了条件

Plan 007の完了は、schema v8の固定10 stage、全raw receipt、wallet/refund/RPC rehearsal、same-instance current-schema upgrade、reactivation、監視postconditionを同じsource commitへ固定し、全pending/liabilityが0かつ資産flowがunpausedの`SHORT_DELAY_LIVE`へ到達した時点とする。v7履歴は遡及的に合格扱いしない。これはproduction activationのblockerではない。10回・7日計測とSNS proposal upgradeはPlan 005/006に残す。x402はBridgeの完了条件に含めない。
