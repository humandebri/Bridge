# Phase 1E 検証義務表

この表は、Phase 1Eで凍結するBase contractの主張と、各主張を確認する検証手段を対応付ける。

| 対象 | 不変条件または主張 | 検証手段 | 境界・外部仮定 |
|---|---|---|---|
| Deposit算術 | `net = gross - fee`、fee上限、Per-Deposit Limit、window消費量の上限を成功経路で維持する | `MintAccounting`を共有するSMT pass fixtureとFoundry unit/fuzz test | EVMのtransaction rollbackとSolidityのchecked arithmeticを前提にする |
| Deposit識別子 | processed IDは成功mintで一度だけ確定し、batch失敗時には状態を変更しない | Foundryのsingle・batch・rollback testとBridge stateful invariant | mappingの永続性とeventログの配信を外部仮定とする |
| ERC-20供給操作 | mintとburnはimmutable Bridgeからだけ呼べ、通常のtransferとallowanceは標準動作を保つ | BSNS unit testとABI snapshot | OpenZeppelin ERC-20実装の正当性とsubmodule revisionを前提にする |
| EIP-3009 | domain、厳密な期限、署名者、high-`s`、nonce namespace、失敗時rollbackを守る | BSNS unit testとauthorization nonce fuzz test | OpenZeppelin EIP712、ECDSA、EIP-5267の実装を外部仮定とする |
| Withdrawal exposure | `totalSupply + Pending amount + Released amount`が成功Deposit mint累計に一致する | `BridgeInvariantTest`のstateful invariantとWithdrawal unit/fuzz test | handlerが追跡する操作範囲外の任意callerを別のunit testで補完する |
| Withdrawal終端 | `Pending`から`Released`または`Refunded`へ一度だけ遷移し、終端recordを再遷移させない | `WithdrawalAccounting` SMT fixture、Foundry stateful invariant、release/refund unit test | mappingの一意性とEVM rollbackを外部仮定とする |
| Release acknowledgement | settlement分解、minimum、fee上限、ledger block index一意性、同一ack冪等性を守る | `WithdrawalAccounting` SMT fixtureとFoundry unit/fuzz test | Bridge Signerが信頼された記録を送ることを前提にする |
| 管理権限 | role分離、fee上限、安全方向limit変更、pause独立性を維持する | `BridgeAdministration` SMT fixture、Foundry administration test、Bridge invariant | role addressが正しいTimelockを指すことはdeploy preflightの責務とする |
| Timelock | 72時間delay、Safe限定proposer・canceller・executor、自己admin制約を維持する | OpenZeppelin revisionを固定したFoundry integration testとlocal smoke | TimelockControllerの内部時刻・AccessControl実装を外部仮定とする |
| ABI | constructor順、struct field順、enum ordinal、function/error/eventのselector・topicを変更しない | canonical concrete ABI snapshot、interface subset checker、selector fixture | Phase 1E後のABI変更は別計画と再レビューを要する |

SMTCheckerは純粋libraryの算術とdecisionだけを証明し、caller、mapping、event、ERC-20、署名、Timelock、block timestamp、transaction rollbackはFoundryと外部仮定で補完する。

Verusの新しい義務はPhase 1Eでは追加せず、既存のpass・fail fixtureで検証器のgateを維持する。
