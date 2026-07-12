# SNS–Base Bridge

SNSトークンをICPとBaseの間で移動し、両チェーンにまたがる裏付け債務を管理する文脈。

## Language

**Deposit**:
ICPでSNSトークンをロックし、Service Feeを引いた量のbSNSをBaseでmintする1件の要求。
_Avoid_: Bridge transaction, transfer

**Withdrawal**:
Baseでbridged tokenをburnし、ICPでSNSトークンをreleaseまたはBaseでrefundする1件の要求。
_Avoid_: Return transfer, redeem transaction

**Bridge Exposure**:
Baseの発行済みbridged tokenと、burn済みだがICP releaseまたはBase refundが未確定のWithdrawalの合計。Bridgeが裏付ける債務総額を表す。
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
WithdrawalがICP ReleaseまたはBase Refundの一方だけで終端した状態。両方の成立を許さない。
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

**Upgrade Authority**:
Bridge canisterのコード更新を承認するSNS Governance。SNS Rootが唯一のcontrollerとして採択済みupgradeを実行する。
_Avoid_: Runtime administrator, developer controller

**Runtime Administrator**:
Bridgeのpause、limitの引き下げ、上限内のService Fee、Fee Recipientだけを操作する外部管理鍵。安全方向の操作に限定され、canister controllerではない。
_Avoid_: Upgrade authority, owner

**Bridge Signer**:
Bridge canisterのthreshold ECDSAで管理され、Baseのdeposit mint、Release acknowledgement、Base Refundだけを実行する単一address。
_Avoid_: Base Admin, Runtime Administrator, owner

**Base Admin**:
Base contractのunpause、limitの引き上げ、role rotationを72時間のOpenZeppelin Timelock経由で承認・実行するSafe multisig。mint、refund、escrow資産への権限を持たない。
_Avoid_: Governance Executor, owner, DEFAULT_ADMIN_ROLE holder
