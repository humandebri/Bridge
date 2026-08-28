# IC mainnet × Base Sepolia staging E2E

このrunbookは、既存staging Canister principalを再利用し、schema v35／record wire v30で`reinstall`する。旧stable stateと旧Base stackの未処理test transactionは引き継がず、事前観測を残して破棄する。旧Base contractsにはpause、upgrade、migrationを行わない。production Canister、KINIC Ledger、Base Mainnet、SNSを対象にしてはならない。

Base Sepolia stagingだけは`short-delay-test-only` policyによりactivation delayを300秒とする。production artifactの24時間制約は変更せず、短縮版artifactと証跡をproduction rehearsalへ使用しない。

## Test Ledgerのfee

stagingは共有TICRC1 Ledger/Indexだけを再利用する。`test-deployment` WasmはTICRC1の`icrc1_fee()`と一致する`KINIC_LEDGER_FEE = 10000`を使い、production Wasmの`100000`とは分離する。staging artifactをproductionへ流用しない。

## 旧stackの切離し記録

`deployments/sepolia-staging/legacy-stack-binding.json`を旧stackの識別情報とする。新stackへ切り替える前に、次をlive queryからrepo外の切離し証跡へ保存する。

- 旧Canister、Bridge、bSNS、Timelock、signer、deployment instanceのexact identity。
- pending Deposit/Withdrawal、reserved mint amount、pending Ledger operation、reconciliation holdの観測値。
- Timelock queueとgovernance transactionの観測値。
- active frontend profile、new signer、new Bridgeのいずれからも旧Base identityと旧deployment instanceを参照しないこと。

旧stackの値はstaging test stateとして放棄し、新stackへ移送、返金、再発行しない。未処理test transactionやunpaused状態は切替blockerにしない。旧Canister stateはreinstallで破棄し、旧Base contractsは変更せずUI、signer、profile、automationから到達不能にする。

## 新stackの構築

`deployments/sepolia-staging/fresh-stack.template.json`をrepo外へコピーし、観測値で埋める。予定値や手入力した成功要約を証跡にしない。

1. clean commitで`scripts/plan007-local-gate.sh /secure/work/local-e2e.json`を実行する。
2. fresh Timelock、Bridge、Bridgeが生成するbSNS、専用Bridge signer、deployment instance IDを作る。
3. 既存IC Bridge Canister `rlhjx-iyaaa-aaaaf-qcnyq-cai`へcanonical artifact `target/test-deployment/staging/bridge_canister.wasm`をschema v35／wire v30で`reinstall`する。raw Cargo artifact、`auto`、`upgrade`は禁止する。
4. 共有TICRC1 Ledger/Indexだけをbindingへ設定し、runtime hash、chain ID、3つの固定RPC、signer、Bridge、bSNS、Timelockを固定する。
5. cycles補充は`cycles-management`手順に従い、`icp canister top-up`後のexecution balanceを確認する。
6. activation後に`frontend-profile.json`を新しいlive観測値へ更新し、同じprofileからUI artifactをbuildする。
7. Cloudflare staging UI公開は、Canisterとcontractのpostcondition確認後にだけ行う。

reinstallとcycles投入、Base Sepolia transaction、Cloudflare公開はそれぞれレビュー可能なstage evidenceを作り、実行前承認を得る。秘密鍵、identity名、RPC credentialはリポジトリへ保存しない。

## 証跡state machine

```text
legacy_abandonment_record
  -> preflight
  -> contracts
  -> destructive_reinstall
  -> initialize
  -> activation_schedule
  -> activation_execute
  -> frontend_publish
  -> smoke_e2e
  -> wallet_e2e
  -> refund_rehearsal
  -> rpc_rehearsal
  -> live_acceptance
  -> SHORT_DELAY_LIVE
```

各stageはsource commit、artifact SHA-256、実行対象、raw receipt、観測postconditionを保存する。失敗commandの出力をPASS証跡にしない。reinstall後は`get_runtime_binding`、`get_operational_config`、`get_bridge_status`、`storage_integrity_check`を再取得し、schema `35`、TTL `600`、new deployment instance、runtime hash、signer、chain/RPC binding、空state、pause状態が計画と一致することを確認する。

## Finality遅延とWallet E2E

- Finalized headを意図的に約20分遅らせた状態でDeposit reviewが成功し、authorizationの`issued_at_timestamp`と`deadline = issued_at + 600`を記録する。
- countdownとsend admissionがlatest Base timestampを使い、残り300秒で送信可能、299秒でtransaction未送信になることを記録する。
- 実walletでTICRC1 DepositからBase mintまで完了し、Base ETH支払receiptとexact processed Depositを記録する。
- 未使用authorizationはdeadline後でもFinalized timestampがdeadlineを超えるまではrefund不可で、超過後のexact未処理証拠でだけrefundできることを記録する。
- Solidityは`block.timestamp + 900`を受理し、`+901`を拒否することを対象artifactとtransaction testで固定する。

## RPC故障演習

RPC順序はPublicNode、`sepolia.base.org`、dRPCに固定し、2-of-3 quorumを確認する。単一provider failureは継続し、quorum loss、chain mismatch、canonical receipt不一致はfail closedにする。runtimeでRPC URLまたはchain IDを可変にしない。

## 終了条件

- status TTL=`600`、Solidity上限=`900`。
- 通常の20〜30分のFinalized遅延下でreviewとwallet Deposit→mintが成功。
- 未使用authorizationのrefund rehearsalがFinalized境界どおり成功。
- old Base stackと旧deployment instanceがactive profile、signer、automationから完全に除外されている。
- new Bridge/bSNS/Timelock、new signer、reused Canister ID、deployment instance、runtime/profile hashが証跡と一致。
- 新stackはlive stagingとしてactiveのまま残し、受入操作以外のpending transactionがない。

Production配置はこのrunbookに含めない。新staging証跡と複数回のBase Mainnet finality測定だけをproduction Gate Bの入力にする。
