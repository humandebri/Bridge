# Plan 006: SNS handover・Canister操作型Base管理・production preflight

## Status

- **State**: IN PROGRESS
- **Dependency**: Plan 005の10回・7日計測、固定limit承認、実pause principal、pause/cancel経路演習が完了していること
- **Safety**: Gate B executeへの明示承認まで本番資産を受け付けない。外部transaction、controller変更、proposal提出、activationは個別承認なしに実行しない。

## 権限モデル

KINIC SNS Governance `74ncn-fqaaa-aaaaq-aaasa-cai`をIC/Base双方の管理trust rootとする。人間が長期保有する管理資格情報は単一のIC emergency pause principalだけとし、finance principal、release approver、人間のBase Admin/Runtime/Cancellerを置かない。

Bridge Canisterは異なるderivation pathからMint SignerとGovernance Operatorを導出する。Mint SignerはEIP-712 Deposit Mint Authorization署名専用で、Base transactionを送信せずETHも保持しない。Governance OperatorはBase pause、Service Fee、Timelock schedule/cancel/executeの署名専用とし、外部relayerが送信・確定通知を担う。nonceとtransaction recordはMint署名レーンと共有しない。Base管理APIはclosed enumのみを受け付け、任意target、calldata、raw transaction、nonceを入力させない。

## 固定stage

1. clean revisionでCI、Verus、ABI/Candid、current schema reopenと未知schema fail-closedを完了する。
2. 同一Wasmのtest canisterで10回計測、launch-ready RPC 5 scenario、実データ相当stateのupgrade、pause/cancel経路演習を完了する。
3. production Canisterへ通常のpause状態で同一Wasmをinstallし、Canister固有のMint SignerとGovernance Operatorを導出する。追加のbootstrap lifecycleやdeployment binding APIは設けない。
4. 最終profile、予測contract address、4 artifactのGate Aを固定する。
5. 一時deployerでTimelockとBridgeをpause状態で配置する。constructorは導出済みMint Signer、Governance Operator、Timelockだけをroleへ設定し、deployerへroleを残さない。
6. この端末のproduction preflightでcanonical receipt、runtime hash、role集合、deployer roleゼロ、pause状態を検証する。
7. controllerをKINIC SNS Root `7jkta-eyaaa-aaaaq-aaarq-cai`一件へhandoverし、SNS proposalによる同一Wasm upgradeを実証する。
8. fresh Gate B後、SNS proposalから引数なしの`schedule_activation`を呼ぶ。Canisterがlive preflightを行い、Governance Operatorで固定された24時間Timelock operationをscheduleする。
9. 24時間後に別のfresh Gate Bを作り、別SNS proposalから引数なしの`execute_activation`を呼ぶ。Canisterがlive preflightを再実行して記録済みoperationだけをexecuteする。
10. Base両flowのcanonical Finalized成功後だけIC Depositを自動resumeする。失敗、曖昧結果、driftではpauseを維持する。

## Evidence契約

Gate Aは`profile.json`、`monitor-drill.json`、`bridge-canister.wasm`、`bridge-runtime.bin`の正確に4件とする。Gate Bはこれらに`signer-snapshot.json`、`rpc-e2e.json`、`controller-handover.json`、`sns-upgrade.json`、`gate-a-receipt.json`を加えた正確に9件とする。鍵ceremonyとrelease approvalは存在しない。Mint Signerはprofile、Canister公開設定、Finalized Base stateの三者一致で検証する。x402はBridgeの配置・activation条件に含めない。

`monitor-drill.json`はpause principal、実request ID、audit sequence、audit digestを含む。Gate B snapshotは投票開始時の承認そのものではなく、schedule/execute時のCanister live preflight結果とSNS proposal実行証跡を別々に保存する。SNS proposal IDとGate B hashはCanisterへ自己申告値として渡さず、SNSとevidence側で管理する。manifestは最大90日、schedule用とexecute用は別bundleとする。

## 完了条件

- 人間の永続EVM roleが0件である。
- SNS Root-only controllerとSNS proposal upgradeが成功している。
- `preflight`、`authorization_mint`、`withdrawal_release`、`quorum_loss`、`final_pause`の主要5 scenarioがraw artifact付きで`LAUNCH_READY`になっている。
- Canister発のTimelock schedule/executeとcanonical Finalized receiptが存在する。
- Base/IC双方がactiveで、controller、code、role、reserveにdriftがない。

EIP-3009はbSNSの任意連携機能であり、外部facilitatorとの互換性はBridgeの本番準備をblockしない。最後の明示承認までは本番資産受付を開始しない。
