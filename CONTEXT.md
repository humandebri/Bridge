# KINIC–Base Bridge

KINICトークンをICPとBaseの間で移動し、両チェーンにまたがる裏付け債務を管理する文脈。

本BridgeはKINIC専用である。以下の`SNSトークン`や`bSNS`は設計上の総称として使い、実際の対象はKINIC、Base上のERC-20 metadataは`name = "kinic"`、`symbol = "KINIC"`とする。

## Language

**Deposit**:
ICPでSNSトークンをロックし、Service Feeを引いた量のbSNSをBaseでmintする1件の要求。
_Avoid_: Bridge transaction, transfer

**Withdrawal**:
Baseでbridged tokenをburnし、ICPでSNSトークンをreleaseする1件の要求。Base refundは提供しない。
_Avoid_: Return transfer, redeem transaction

**Bridge Exposure**:
Baseの発行済みbridged tokenと、burn済みだがICP releaseが未確定のWithdrawalの合計。Bridgeが裏付ける債務総額を表す。
_Avoid_: Outstanding supply, moved amount

**Per-Deposit Limit**:
1件のDepositとして受理できる最大量。累積移動量やBridge Exposureの上限ではない。
_Avoid_: Supply cap, bridge cap

**Mint Throughput Limit**:
一定時間にdeposit mintできる量を制御する流量上限。全量移動を禁止せず、短時間の被害拡大を抑える。
_Avoid_: Daily supply cap, outstanding cap

**Bridgeable SNS Token**:
ユーザーが移動可能なledger accountに保有する、neuronへstakeされていないSNSトークン。
_Avoid_: Total supply, staked token

**bSNS**:
Bridgeable SNS Tokenを1:1で裏付けるBase上のERC-20。SNS Governanceの投票権やneuron権限を持たない。
これは内部の総称であり、ERC-20 metadataへ`b` prefixを付けることを意味しない。KINIC用deployのtoken nameは`kinic`、symbolは`KINIC`とする。
_Avoid_: Cross-chain governance token, voting token

**Withdrawal Settlement**:
WithdrawalのICP ReleaseがLedger成功または履歴照合で確定し、`Paid`になった状態。Base refundは存在せず、`Committed`後のBase stateは終端である。
_Avoid_: Withdrawal completion, payout status

**Service Fee**:
成功したDepositまたはWithdrawalごとにSNSトークンで徴収する固定手数料。送金額に比例せず、デプロイ時に固定した上限を超えない。
_Avoid_: Gas fee, percentage fee, spread

**Fee Recipient**:
確定済みService Feeを受け取る現在のSNS ledger account。管理者が変更すると、未送金fee reserve全体の送付先も新accountへ変わる。
_Avoid_: Treasury owner, escrow owner

**Settlement Reserve**:
単一のBridge資源残高のうち、既存Withdrawal Settlementの完了を優先するため論理的に予約した部分。
_Avoid_: Settlement wallet, separate treasury

**Reconciliation Hold**:
外部transferの成否を確定できず、二重処理を避けるため補償操作を禁止した状態。時間経過だけでは解除しない。
_Avoid_: Timed out, failed, retryable

**Deposit Cancellation**:
DepositのICP pullが完全な履歴scanにより存在しないと証明された後、そのDeposit IDを再利用不能にする終端結果。token移動やService Fee確定を伴わない。
_Avoid_: Retry, timeout, reopen

**Withdrawal Transfer Attempt**:
1件のWithdrawalに対する特定のICRC release identity。不存在が完全に証明された場合だけ番号を増やし、経済的payloadを維持した新identityへ更新できる。
_Avoid_: Withdrawal retry, replacement withdrawal

**Ledger History Watermark**:
Reconciliationで完全性を確認したinclusiveなledger block index。最初の未走査blockがledger tipを超え、archiveとindexを含むexact transfer探索が完了した場合だけ不存在証拠になる。
_Avoid_: Timestamp, timeout, last attempted block

**Upgrade Authority**:
Bridge canisterのコード更新を承認するSNS Governance。SNS Rootが唯一のcontrollerとして採択済みupgradeを実行する。
_Avoid_: Runtime administrator, developer controller

**Governance Principal**:
Upgrade Authorityと同じSNS Governance principal。Canister上でDeposit受付の再開、runtime administrator rotation、許可されたSettlementの進行、reverted EVM operationの証拠付きrecoveryを実行する。Base contractのRuntime AdministratorまたはBase Adminではない。
_Avoid_: Upgrade controller, Runtime Administrator, Base Admin

**Pause Principal**:
新規Depositの即時停止と許可されたSettlementの進行だけを行う複数のIC principal。hardware pause principalはこのroleを保持する物理鍵であり、再開、role rotation、fee管理、upgradeを行わない。
_Avoid_: Governance Principal, Runtime Administrator, canister controller

**Finance Administrator**:
Fee Recipient変更とfee payoutだけを行うIC principal。pause、recovery、runtime administrator rotation、upgrade権限を持たない。
_Avoid_: Treasury owner, Governance Principal, Base Admin

**Runtime Administrator**:
Base Bridgeのpauseと上限内Service Fee変更だけを操作する外部管理鍵。canister controllerではない。
_Avoid_: Upgrade authority, owner

**Bridge Signer**:
Bridge canisterのthreshold ECDSAで管理され、BaseのDeposit mintだけを実行する単一address。Withdrawalのburnは利用者が実行する。
_Avoid_: Base Admin, Runtime Administrator, owner

**Base Admin**:
Base contractのunpauseとrole rotationを72時間のOpenZeppelin Timelock経由で承認・実行する単一hardware wallet。mint、Withdrawal操作、limit変更、escrow資産への権限を持たない。
_Avoid_: Governance Executor, owner, DEFAULT_ADMIN_ROLE holder
