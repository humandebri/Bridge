# Plan 006: SNS handover・Canister操作型Base管理・production preflight

## Status

- **State**: IN PROGRESS
- **Dependency**: Plan 005の初期運用値、固定limit、実pause principalが確定していること。pause/cancel経路演習と7日・各10件の本番計測はunpause後のGate Cで行い、初回activationまたはcontroller handoverを認可しない。
- **Safety**: Gate B executeへの明示承認まで本番資産を受け付けない。外部transaction、controller変更、proposal提出、activationは個別承認なしに実行しない。

## 権限モデル

KINIC SNS Governance `74ncn-fqaaa-aaaaq-aaasa-cai`をIC/Base双方の管理trust rootとする。人間が長期保有する管理資格情報は単一のIC emergency pause principalだけとし、finance principal、release approver、人間のBase Admin/Runtime/Cancellerを置かない。

Bridge Canisterは異なるderivation pathからMint SignerとGovernance Operatorを導出する。Mint SignerはEIP-712 Deposit Mint Authorization署名専用で、Base transactionを送信せずETHも保持しない。Governance OperatorはBase pause、Service Fee、Timelock schedule/cancel/executeの署名専用とし、外部relayerが送信・確定通知を担う。nonceとtransaction recordはMint署名レーンと共有しない。Base管理APIはclosed enumのみを受け付け、任意target、calldata、raw transaction、nonceを入力させない。

## 固定stage

1. clean revisionでCI、Verus、ABI/Candid、current schema reopenと未知schema fail-closedを完了する。
2. 同一Wasmのtest canisterで実データ相当stateのupgrade、PocketIC、proofのpre-activation安全証拠を完了する。10回計測、launch-ready RPC 5 scenario、pause/cancel経路演習はunpause後のGate Cへ分離する。
3. production Canisterへpause状態でcontroller-bootstrap Wasmをinstallし、Canister固有のMint SignerとGovernance Operatorを導出する。既存Candid method／argument ABIを維持し、承認済みの`ActivationConfirmationView` 2 field以外に公開APIを増やさず、初回activation専用の内部bootstrap lifecycleを使う。
4. 最終pre-deploy profileとBridge／BSNSの5 build artifact、合計6 artifactのGate Aを固定する。
5. 一時deployerでTimelockとBridgeをpause状態で配置する。constructorは導出済みMint Signer、Governance Operator、Timelockだけをroleへ設定し、deployerへroleを残さない。
6. この端末のproduction preflightでcanonical receipt、runtime hash、role集合、deployer roleゼロ、pause状態を検証する。
7. pre-seal Gate B後、production controllerが初期運用値を一度だけsealする。
8. fresh live Gate B後、production controllerが`schedule_activation`をprepareし、匿名relayと固定confirmation relayer confirmで24時間Timelock operationをscheduleする。
9. 24時間後に別のfresh live Gate Bを作り、production controllerが`execute_activation`をprepareし、同じ役割分離で記録済みoperationだけをexecuteする。
10. Base両flowのcanonical Finalized成功後だけIC Depositを自動resumeし、内部bootstrap activation authorityを永久に消費する。失敗、曖昧結果、driftではpauseを維持する。
11. unpause後の7日・各10件以上の本番計測／Gate Cとcontroller handoverは独立に扱う。handoverは自動実行せず、運用者が時期を別途承認した場合だけSNS Root一件へ変更してSNS proposal upgradeを実証する。

## Evidence契約

Gate Aは配置済みartifactとして不変に保持する。pre-seal Gate Bは初期運用値と構造証跡、live Gate Bはcertified config、attestation、sole production controller、module／pause／reserve／cycles／pending状態を検証する。Gate Cはunpause後の本番計測、monitoring、keeper、upgrade履歴を追加する。鍵ceremonyとrelease approvalは存在しない。Mint Signerはprofile、認証済みCanister公開設定、freshなFinalized Base attestationの三者一致で検証する。x402はBridgeの配置・activation条件に含めない。

`monitor-drill.json`はpause principal、実request ID、audit sequence、audit digestを含む。Gate Bのfresh attestationは初回activationの承認そのものではなく、controller activation authorization、prepare、confirmation、Finalized statusを別々のreceiptへ保存する。SNS proposal IDと実行証跡はhandover後の再activationにだけ使用し、Gate B hashとともにCanisterへ自己申告値として渡さない。manifestは最大90日とし、schedule／executeは同一Gate B bundleにphase別authorization、artifact、receiptを接続する。

## 完了条件

- 人間の永続EVM roleが0件である。
- 初回schedule／executeのcontroller activation receiptが揃い、内部bootstrap authorityが消費されている。
- unpause後にGate Cを実施した場合は、`preflight`、`authorization_mint`、`withdrawal_release`、`quorum_loss`、`final_pause`の主要5 scenarioがraw artifact付きで`LAUNCH_READY`になっている。ただし初回activationまたはcontroller handoverの完了条件にはしない。
- Canister発のTimelock schedule/executeとcanonical Finalized receiptが存在する。
- Base/IC双方がactiveで、controller、code、role、reserveにdriftがない。
- handoverを別途実行した場合だけ、SNS Root-only controllerとSNS proposal upgradeが成功している。

EIP-3009はbSNSの任意連携機能であり、外部facilitatorとの互換性はBridgeの本番準備をblockしない。初回executeの明示承認までは本番資産受付を開始せず、handoverの明示承認は別に扱う。
